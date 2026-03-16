use std::path::Path;

use miette::{Context, IntoDiagnostic, Result};
use serde::Serialize;

use super::{
    classify_error, create_muxer, open_demuxer, parse_time, validate_output_format, ErrorResult,
    OutputFormat,
};

// ---------------------------------------------------------------------------
// Trim types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct TrimResult {
    #[serde(rename = "type")]
    event_type: &'static str,
    input: String,
    output: String,
    packets_written: u64,
    packets_skipped: u64,
    actual_start_seconds: Option<f64>,
    actual_end_seconds: Option<f64>,
}

// ---------------------------------------------------------------------------
// Trim command
// ---------------------------------------------------------------------------

pub(crate) fn trim(
    input: &Path,
    output: &Path,
    start: Option<&str>,
    end: Option<&str>,
    format: &OutputFormat,
) -> Result<()> {
    let json_mode = matches!(format, OutputFormat::Json);

    let result = trim_inner(input, output, start, end, format);

    if json_mode {
        if let Err(e) = result {
            let (error_kind, code) = classify_error(&e);
            let error_json = ErrorResult {
                event_type: "error",
                error_kind: error_kind.to_string(),
                message: format!("{e}"),
                input: Some(input.display().to_string()),
            };
            println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
            std::process::exit(code);
        }
    }

    result
}

fn trim_inner(
    input: &Path,
    output: &Path,
    start: Option<&str>,
    end: Option<&str>,
    format: &OutputFormat,
) -> Result<()> {
    validate_output_format(output)?;
    let start_secs = start.map(parse_time).transpose()?;
    let end_secs = end.map(parse_time).transpose()?;

    let mut demuxer = open_demuxer(input)?;
    let mut muxer = create_muxer(output)?;

    let track_count = demuxer.tracks().len();
    for i in 0..track_count {
        let info = demuxer.tracks()[i].clone();
        muxer
            .add_track(&info)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to add track {i}"))?;
    }

    let mut packet_count: u64 = 0;
    let mut skipped: u64 = 0;

    // Track the last keyframe before start for each track, so the output
    // begins on a keyframe and decoders can produce valid frames.
    let mut pending_keyframes: std::collections::HashMap<u32, splica_core::Packet> =
        std::collections::HashMap::new();
    let mut past_start = start_secs.is_none();
    let mut actual_start_secs: Option<f64> = None;
    let mut actual_end_secs: Option<f64> = None;

    while let Some(packet) = demuxer
        .read_packet()
        .into_diagnostic()
        .wrap_err("failed to read packet")?
    {
        let pts_secs = packet.pts.as_seconds_f64();

        // Before start: buffer the last keyframe per track
        if let Some(start) = start_secs {
            if !past_start && pts_secs < start {
                if packet.is_keyframe {
                    pending_keyframes.insert(packet.track_index.0, packet);
                }
                skipped += 1;
                continue;
            }

            // We've crossed the start boundary — flush buffered keyframes first
            if !past_start {
                past_start = true;

                // Write the last keyframe for each track (snap to keyframe)
                let mut keyframes: Vec<_> = pending_keyframes.drain().collect();
                keyframes.sort_by_key(|(idx, _)| *idx);
                for (_, kf_packet) in keyframes {
                    if actual_start_secs.is_none() {
                        actual_start_secs = Some(kf_packet.pts.as_seconds_f64());
                    }
                    muxer
                        .write_packet(&kf_packet)
                        .into_diagnostic()
                        .wrap_err("failed to write keyframe packet")?;
                    packet_count += 1;
                }
            }
        }

        if let Some(end) = end_secs {
            if pts_secs >= end {
                skipped += 1;
                continue;
            }
        }

        actual_end_secs = Some(pts_secs);

        muxer
            .write_packet(&packet)
            .into_diagnostic()
            .wrap_err("failed to write packet")?;
        packet_count += 1;
    }

    muxer
        .finalize()
        .into_diagnostic()
        .wrap_err("failed to finalize output")?;

    match format {
        OutputFormat::Json => {
            let result = TrimResult {
                event_type: "complete",
                input: input.display().to_string(),
                output: output.display().to_string(),
                packets_written: packet_count,
                packets_skipped: skipped,
                actual_start_seconds: actual_start_secs,
                actual_end_seconds: actual_end_secs,
            };
            let json = serde_json::to_string_pretty(&result).into_diagnostic()?;
            println!("{json}");
        }
        OutputFormat::Text => {
            eprintln!(
                "Trimmed: wrote {packet_count} packets, skipped {skipped} to {}",
                output.display()
            );
            if let (Some(s), Some(e)) = (start_secs, end_secs) {
                if let Some(actual) = actual_start_secs {
                    if (actual - s).abs() > 0.01 {
                        eprintln!("  Trimmed from {actual:.2}s (snapped from {s:.2}s to nearest keyframe) — {e:.2}s");
                    } else {
                        eprintln!("  Time range: {s:.2}s — {e:.2}s");
                    }
                } else {
                    eprintln!("  Time range: {s:.2}s — {e:.2}s");
                }
            } else if let Some(s) = start_secs {
                if let Some(actual) = actual_start_secs {
                    if (actual - s).abs() > 0.01 {
                        eprintln!(
                            "  Trimmed from {actual:.2}s (snapped from {s:.2}s to nearest keyframe)"
                        );
                    } else {
                        eprintln!("  Start: {s:.2}s");
                    }
                } else {
                    eprintln!("  Start: {s:.2}s");
                }
            } else if let Some(e) = end_secs {
                eprintln!("  End: {e:.2}s");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_trim_result_serializes_type_complete() {
        let result = TrimResult {
            event_type: "complete",
            input: "input.mp4".to_string(),
            output: "output.mp4".to_string(),
            packets_written: 10,
            packets_skipped: 5,
            actual_start_seconds: Some(0.1),
            actual_end_seconds: Some(1.0),
        };

        let json: serde_json::Value = serde_json::to_value(&result).unwrap();

        assert_eq!(json["type"], "complete");
    }
}
