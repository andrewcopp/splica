use std::path::Path;

use miette::{Context, IntoDiagnostic, Result};
use serde::Serialize;

use splica_core::{Demuxer, Packet, Timestamp};

use super::{
    classify_error, create_muxer, open_demuxer, validate_output_format, ErrorResult, OutputFormat,
};

// ---------------------------------------------------------------------------
// Join types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JoinResult {
    #[serde(rename = "type")]
    event_type: &'static str,
    inputs: Vec<String>,
    output: String,
    files_joined: usize,
    packets_written: u64,
}

// ---------------------------------------------------------------------------
// Join command
// ---------------------------------------------------------------------------

pub(crate) fn join(inputs: &[&Path], output: &Path, format: &OutputFormat) -> Result<()> {
    let json_mode = matches!(format, OutputFormat::Json);

    let result = join_inner(inputs, output, format);

    if json_mode {
        if let Err(e) = result {
            let (error_kind, code) = classify_error(&e);
            let error_json = ErrorResult {
                event_type: "error",
                error_kind: error_kind.to_string(),
                message: format!("{e}"),
                input: None,
            };
            println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
            std::process::exit(code);
        }
    }

    result
}

fn join_inner(inputs: &[&Path], output: &Path, format: &OutputFormat) -> Result<()> {
    if inputs.len() < 2 {
        return Err(miette::miette!(
            "join requires at least 2 input files, got {}",
            inputs.len()
        ));
    }

    validate_output_format(output)?;

    // Open first demuxer to establish the reference track layout
    let first_demuxer = open_demuxer(inputs[0])?;
    let reference_tracks = first_demuxer.tracks().to_vec();
    drop(first_demuxer);

    // Validate all inputs share the same track layout and codecs
    for (i, &input_path) in inputs.iter().enumerate().skip(1) {
        let demuxer = open_demuxer(input_path)?;
        validate_tracks_match(
            &reference_tracks,
            demuxer.tracks(),
            inputs[0],
            input_path,
            i,
        )?;
    }

    // Set up muxer
    let mut muxer = create_muxer(output)?;
    for track in &reference_tracks {
        muxer
            .add_track(track)
            .into_diagnostic()
            .wrap_err("failed to add track")?;
    }

    let mut packet_count: u64 = 0;
    let mut cumulative_duration = Timestamp::new(0, 90_000).expect("90kHz timebase is valid");

    // Process each input file sequentially
    for &input_path in inputs {
        let mut demuxer = open_demuxer(input_path)?;
        let mut max_pts = cumulative_duration;

        while let Some(packet) = demuxer
            .read_packet()
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to read packet from '{}'", input_path.display()))?
        {
            let offset_packet = offset_packet(&packet, cumulative_duration)?;

            // Track the maximum PTS to compute cumulative duration
            if offset_packet.pts > max_pts {
                max_pts = offset_packet.pts;
            }

            muxer
                .write_packet(&offset_packet)
                .into_diagnostic()
                .wrap_err("failed to write packet")?;
            packet_count += 1;
        }

        // Update cumulative duration from track metadata if available,
        // otherwise fall back to the max PTS seen
        let file_duration = file_duration_from_tracks(demuxer.tracks());
        cumulative_duration = match file_duration {
            Some(dur) => cumulative_duration.checked_add(dur).ok_or_else(|| {
                miette::miette!("timestamp overflow computing cumulative duration")
            })?,
            None => max_pts,
        };
    }

    muxer
        .finalize()
        .into_diagnostic()
        .wrap_err("failed to finalize output")?;

    let input_strings: Vec<String> = inputs.iter().map(|p| p.display().to_string()).collect();

    match format {
        OutputFormat::Json => {
            let result = JoinResult {
                event_type: "complete",
                inputs: input_strings,
                output: output.display().to_string(),
                files_joined: inputs.len(),
                packets_written: packet_count,
            };
            let json = serde_json::to_string_pretty(&result).into_diagnostic()?;
            println!("{json}");
        }
        OutputFormat::Text => {
            eprintln!(
                "Joined {} files: wrote {packet_count} packets to {}",
                inputs.len(),
                output.display()
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validates that the tracks in a subsequent file match the reference file.
fn validate_tracks_match(
    reference: &[splica_core::TrackInfo],
    candidate: &[splica_core::TrackInfo],
    ref_path: &Path,
    cand_path: &Path,
    file_index: usize,
) -> Result<()> {
    if reference.len() != candidate.len() {
        return Err(miette::miette!(
            "track count mismatch: '{}' has {} tracks but '{}' (file {}) has {} tracks\n  \
             → All input files must have the same number of tracks",
            ref_path.display(),
            reference.len(),
            cand_path.display(),
            file_index + 1,
            candidate.len(),
        ));
    }

    for (i, (ref_track, cand_track)) in reference.iter().zip(candidate.iter()).enumerate() {
        if ref_track.codec != cand_track.codec {
            return Err(miette::miette!(
                "codec mismatch on track {i}: '{}' uses {} but '{}' uses {}\n  \
                 → All input files must use the same codecs for concatenation",
                ref_path.display(),
                ref_track.codec,
                cand_path.display(),
                cand_track.codec,
            ));
        }
        if ref_track.kind != cand_track.kind {
            return Err(miette::miette!(
                "track kind mismatch on track {i}: '{}' is {:?} but '{}' is {:?}\n  \
                 → All input files must have the same track layout",
                ref_path.display(),
                ref_track.kind,
                cand_path.display(),
                cand_track.kind,
            ));
        }
    }

    Ok(())
}

/// Offsets a packet's PTS and DTS by the given duration.
fn offset_packet(packet: &Packet, offset: Timestamp) -> Result<Packet> {
    let pts = packet
        .pts
        .checked_add(offset)
        .ok_or_else(|| miette::miette!("PTS overflow when offsetting timestamps"))?;
    let dts = packet
        .dts
        .checked_add(offset)
        .ok_or_else(|| miette::miette!("DTS overflow when offsetting timestamps"))?;

    Ok(Packet {
        track_index: packet.track_index,
        pts,
        dts,
        is_keyframe: packet.is_keyframe,
        data: packet.data.clone(),
    })
}

/// Computes the maximum duration across all tracks in a demuxer.
fn file_duration_from_tracks(tracks: &[splica_core::TrackInfo]) -> Option<Timestamp> {
    tracks.iter().filter_map(|t| t.duration).max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_join_result_serializes_type_complete() {
        let result = JoinResult {
            event_type: "complete",
            inputs: vec!["a.mp4".to_string(), "b.mp4".to_string()],
            output: "out.mp4".to_string(),
            files_joined: 2,
            packets_written: 100,
        };

        let json: serde_json::Value = serde_json::to_value(&result).unwrap();

        assert_eq!(json["type"], "complete");
    }
}
