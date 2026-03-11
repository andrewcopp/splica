use std::path::Path;

use miette::{IntoDiagnostic, Result};
use serde::Serialize;

use splica_core::TrackKind;

use super::{classify_error, open_demuxer, ErrorResult, OutputFormat};

// ---------------------------------------------------------------------------
// Probe types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ProbeOutput {
    file: String,
    tracks: Vec<ProbeTrack>,
}

#[derive(Serialize)]
struct ProbeTrack {
    index: u32,
    kind: String,
    codec: String,
    duration_seconds: Option<f64>,
    width: Option<u32>,
    height: Option<u32>,
    frame_rate: Option<String>,
    pixel_format: Option<String>,
    color_space: Option<String>,
    sample_rate: Option<u32>,
    channels: Option<u32>,
    channel_layout: Option<String>,
}

// ---------------------------------------------------------------------------
// Probe command
// ---------------------------------------------------------------------------

pub(crate) fn probe(file: &Path, format: &OutputFormat) -> Result<()> {
    let json_mode = matches!(format, OutputFormat::Json);

    let result = probe_inner(file, format);

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

fn probe_inner(file: &Path, format: &OutputFormat) -> Result<()> {
    let demuxer = open_demuxer(file)?;

    let tracks: Vec<ProbeTrack> = demuxer
        .tracks()
        .iter()
        .map(|t| {
            let codec = t.codec.to_string();

            let kind = match t.kind {
                TrackKind::Video => "video",
                TrackKind::Audio => "audio",
            };

            ProbeTrack {
                index: t.index.0,
                kind: kind.to_string(),
                codec,
                duration_seconds: t.duration.map(|d| d.as_seconds_f64()),
                width: t.video.as_ref().map(|v| v.width),
                height: t.video.as_ref().map(|v| v.height),
                frame_rate: t
                    .video
                    .as_ref()
                    .and_then(|v| v.frame_rate.map(|fr| fr.to_string())),
                pixel_format: t
                    .video
                    .as_ref()
                    .and_then(|v| v.pixel_format.map(|pf| format!("{pf:?}"))),
                color_space: t
                    .video
                    .as_ref()
                    .and_then(|v| v.color_space.map(|cs| format_color_space(&cs))),
                sample_rate: t.audio.as_ref().map(|a| a.sample_rate),
                channels: t
                    .audio
                    .as_ref()
                    .and_then(|a| a.channel_layout.map(|cl| cl.channel_count())),
                channel_layout: t
                    .audio
                    .as_ref()
                    .and_then(|a| a.channel_layout.map(|cl| format!("{cl:?}"))),
            }
        })
        .collect();

    match format {
        OutputFormat::Json => {
            let output = ProbeOutput {
                file: file.display().to_string(),
                tracks,
            };
            let json = serde_json::to_string_pretty(&output).into_diagnostic()?;
            println!("{json}");
        }
        OutputFormat::Text => {
            println!("File: {}", file.display());
            println!("Tracks: {}", tracks.len());
            println!();
            for track in &tracks {
                println!("  Track {} ({}): {}", track.index, track.kind, track.codec);
                if let (Some(w), Some(h)) = (track.width, track.height) {
                    print!("    Resolution: {w}x{h}");
                    if let Some(ref fr) = track.frame_rate {
                        print!(" @ {fr} fps");
                    }
                    println!();
                }
                if let Some(ref pf) = track.pixel_format {
                    println!("    Pixel format: {pf}");
                }
                if let Some(ref cs) = track.color_space {
                    println!("    Color space: {cs}");
                }
                if let Some(sr) = track.sample_rate {
                    print!("    Sample rate: {sr} Hz");
                    if let Some(ch) = track.channels {
                        print!(", {ch}ch");
                    }
                    if let Some(ref cl) = track.channel_layout {
                        print!(" ({cl})");
                    }
                    println!();
                }
                if let Some(dur) = track.duration_seconds {
                    let mins = (dur / 60.0).floor() as u64;
                    let secs = dur % 60.0;
                    println!("    Duration: {mins}:{secs:05.2}");
                }
            }
        }
    }

    Ok(())
}

fn format_color_space(cs: &splica_core::ColorSpace) -> String {
    use splica_core::media::{ColorPrimaries, ColorRange, TransferCharacteristics};

    let name = match (cs.primaries, cs.transfer) {
        (ColorPrimaries::Bt709, TransferCharacteristics::Bt709) => "bt709",
        (ColorPrimaries::Bt2020, TransferCharacteristics::Smpte2084) => "bt2020-pq",
        (ColorPrimaries::Bt2020, TransferCharacteristics::HybridLogGamma) => "bt2020-hlg",
        (ColorPrimaries::Bt2020, _) => "bt2020",
        _ => "unknown",
    };

    let range = match cs.range {
        ColorRange::Full => "full",
        ColorRange::Limited => "limited",
    };

    format!("{name}/{range}")
}
