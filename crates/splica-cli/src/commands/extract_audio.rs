use std::path::Path;

use miette::{Context, IntoDiagnostic, Result};

use splica_core::TrackKind;

use super::{create_muxer, open_demuxer, validate_output_format};

// ---------------------------------------------------------------------------
// Extract audio command
// ---------------------------------------------------------------------------

pub(crate) fn extract_audio(input: &Path, output: &Path) -> Result<()> {
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

    eprintln!(
        "Extracted {packet_count} audio packets ({} audio tracks) to {}",
        audio_tracks.len(),
        output.display()
    );

    Ok(())
}
