use std::path::Path;

use miette::{Context, IntoDiagnostic, Result};
use serde::Serialize;

use splica_core::TrackKind;

use super::{
    classify_error, create_muxer, open_demuxer, validate_output_format, ErrorResult, OutputFormat,
};

// ---------------------------------------------------------------------------
// Extract audio types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ExtractAudioResult {
    #[serde(rename = "type")]
    event_type: &'static str,
    input: String,
    output: String,
    audio_tracks: usize,
    packets_written: u64,
}

// ---------------------------------------------------------------------------
// Extract audio command
// ---------------------------------------------------------------------------

pub(crate) fn extract_audio(input: &Path, output: &Path, format: &OutputFormat) -> Result<()> {
    let json_mode = matches!(format, OutputFormat::Json);

    let result = extract_audio_inner(input, output, format);

    if json_mode {
        if let Err(e) = result {
            let (error_kind, code) = classify_error(&e);
            let error_json = ErrorResult {
                event_type: "error",
                error_kind: error_kind.to_string(),
                message: format!("{e}"),
            };
            println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
            std::process::exit(code);
        }
    }

    result
}

fn extract_audio_inner(input: &Path, output: &Path, format: &OutputFormat) -> Result<()> {
    validate_output_format(output)?;
    let mut demuxer = open_demuxer(input)?;

    // Find audio tracks
    let audio_tracks: Vec<(usize, splica_core::TrackInfo)> = demuxer
        .tracks()
        .iter()
        .enumerate()
        .filter(|(_, t)| t.kind == TrackKind::Audio)
        .map(|(i, t)| (i, t.clone()))
        .collect();

    if audio_tracks.is_empty() {
        return Err(miette::miette!(
            "no audio tracks found in '{}'",
            input.display()
        ));
    }

    let mut muxer = create_muxer(output)?;

    // Map input track indices to output track indices
    let mut input_to_output: std::collections::HashMap<u32, splica_core::TrackIndex> =
        std::collections::HashMap::new();

    for (input_idx, info) in &audio_tracks {
        let output_idx = muxer
            .add_track(info)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to add audio track {input_idx}"))?;
        input_to_output.insert(*input_idx as u32, output_idx);
    }

    let mut packet_count: u64 = 0;

    while let Some(mut packet) = demuxer
        .read_packet()
        .into_diagnostic()
        .wrap_err("failed to read packet")?
    {
        if let Some(&output_idx) = input_to_output.get(&packet.track_index.0) {
            packet.track_index = output_idx;
            muxer
                .write_packet(&packet)
                .into_diagnostic()
                .wrap_err("failed to write packet")?;
            packet_count += 1;
        }
        // Silently skip non-audio packets
    }

    muxer
        .finalize()
        .into_diagnostic()
        .wrap_err("failed to finalize output")?;

    match format {
        OutputFormat::Json => {
            let result = ExtractAudioResult {
                event_type: "complete",
                input: input.display().to_string(),
                output: output.display().to_string(),
                audio_tracks: audio_tracks.len(),
                packets_written: packet_count,
            };
            let json = serde_json::to_string_pretty(&result).into_diagnostic()?;
            println!("{json}");
        }
        OutputFormat::Text => {
            eprintln!(
                "Extracted {packet_count} audio packets ({} audio tracks) to {}",
                audio_tracks.len(),
                output.display()
            );
        }
    }

    Ok(())
}
