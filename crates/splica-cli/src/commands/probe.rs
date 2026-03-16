use std::fs;
use std::path::Path;

use miette::{IntoDiagnostic, Result};
use serde::Serialize;

use splica_core::TrackKind;

use super::{
    classify_error, detect_format, open_demuxer, DetectedFormat, ErrorResult, OutputFormat,
};

// ---------------------------------------------------------------------------
// Probe types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ProbeOutput {
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bitrate_kbps: Option<u64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color_primaries: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transfer_characteristics: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matrix_coefficients: Option<String>,
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
                input: Some(file.display().to_string()),
            };
            println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
            std::process::exit(code);
        }
    }

    result
}

fn probe_inner(file: &Path, format: &OutputFormat) -> Result<()> {
    let container = detect_container(file);
    let size_bytes = fs::metadata(file).ok().map(|m| m.len());

    let demuxer = open_demuxer(file)?;

    let tracks: Vec<ProbeTrack> = demuxer
        .tracks()
        .iter()
        .map(|t| {
            let codec = t.codec.to_string();

            let kind = match t.kind {
                TrackKind::Video => "video",
                TrackKind::Audio => "audio",
                TrackKind::Subtitle => "subtitle",
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
                profile: t.video.as_ref().and_then(|v| v.profile.clone()),
                level: t.video.as_ref().and_then(|v| v.level.clone()),
                color_primaries: t.video.as_ref().and_then(|v| v.color_primaries.clone()),
                transfer_characteristics: t
                    .video
                    .as_ref()
                    .and_then(|v| v.transfer_characteristics.clone()),
                matrix_coefficients: t.video.as_ref().and_then(|v| v.matrix_coefficients.clone()),
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

    let duration_seconds = max_track_duration(&tracks);
    let bitrate_kbps = compute_bitrate_kbps(size_bytes, duration_seconds);

    match format {
        OutputFormat::Json => {
            let output = ProbeOutput {
                file: file.display().to_string(),
                container,
                duration_seconds,
                size_bytes,
                bitrate_kbps,
                tracks,
            };
            let json = serde_json::to_string_pretty(&output).into_diagnostic()?;
            println!("{json}");
        }
        OutputFormat::Text => {
            println!("File: {}", file.display());
            print_summary(&container, duration_seconds, size_bytes, bitrate_kbps);
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
                if let Some(ref profile) = track.profile {
                    print!("    Profile: {profile}");
                    if let Some(ref level) = track.level {
                        print!(", Level: {level}");
                    }
                    println!();
                }
                if let Some(ref cp) = track.color_primaries {
                    println!("    Color primaries: {cp}");
                }
                if let Some(ref tc) = track.transfer_characteristics {
                    println!("    Transfer: {tc}");
                }
                if let Some(ref mc) = track.matrix_coefficients {
                    println!("    Matrix: {mc}");
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

// ---------------------------------------------------------------------------
// Container-level helpers
// ---------------------------------------------------------------------------

/// Detects the container format name from magic bytes.
fn detect_container(file: &Path) -> Option<String> {
    let mut f = std::fs::File::open(file).ok()?;
    let detected = detect_format(&mut f).ok()?;
    let name = match detected {
        DetectedFormat::Mp4 => "mp4",
        DetectedFormat::WebM => "webm",
        DetectedFormat::Mkv => "mkv",
    };
    Some(name.to_string())
}

/// Returns the maximum duration across all tracks.
fn max_track_duration(tracks: &[ProbeTrack]) -> Option<f64> {
    tracks
        .iter()
        .filter_map(|t| t.duration_seconds)
        .fold(None, |acc, d| Some(acc.map_or(d, |a: f64| a.max(d))))
}

/// Computes overall bitrate in kbps from file size and duration.
fn compute_bitrate_kbps(size_bytes: Option<u64>, duration: Option<f64>) -> Option<u64> {
    let size = size_bytes?;
    let dur = duration.filter(|&d| d > 0.0)?;
    Some((size as f64 * 8.0 / dur / 1000.0) as u64)
}

/// Prints a container/duration/size summary line for text mode.
fn print_summary(
    container: &Option<String>,
    duration: Option<f64>,
    size_bytes: Option<u64>,
    bitrate_kbps: Option<u64>,
) {
    let mut parts = Vec::new();

    if let Some(ref c) = container {
        parts.push(format!("Container: {c}"));
    }
    if let Some(dur) = duration {
        let mins = (dur / 60.0).floor() as u64;
        let secs = dur % 60.0;
        parts.push(format!("Duration: {mins}:{secs:05.2}"));
    }
    if let Some(size) = size_bytes {
        parts.push(format_file_size(size));
    }
    if let Some(kbps) = bitrate_kbps {
        parts.push(format!("{kbps} kbps"));
    }

    if !parts.is_empty() {
        println!("{}", parts.join(", "));
    }
}

/// Formats a byte count into a human-readable string.
fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
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
