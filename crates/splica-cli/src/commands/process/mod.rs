mod args;
mod reencode;
mod stream_copy;
mod wiring;

use std::io::Read as _;
use std::time::Instant;

use miette::Result;
use sha2::{Digest, Sha256};

pub(crate) use args::ProcessArgs;

use super::{
    classify_error, output_container, validate_output_format, AudioCodecArg, AudioMode,
    CompleteEvent, ErrorResult, OutputFormat, TranscodeAudioInfo,
};

struct TranscodeOutput {
    packets_read: u64,
    frames_decoded: u64,
    frames_encoded: u64,
    packets_written: u64,
    audio_tracks: Vec<TranscodeAudioInfo>,
    mux_ok: bool,
    output_codec: Option<String>,
    output_duration_secs: Option<f64>,
    output_bitrate_kbps: Option<u64>,
    is_stream_copy: bool,
    elapsed_secs: f64,
}

fn compute_sha256(path: &std::path::Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| miette::miette!("failed to open output for hashing: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| miette::miette!("failed to read output for hashing: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn process(args: &ProcessArgs<'_>, format: &OutputFormat) -> Result<()> {
    let json_mode = matches!(format, OutputFormat::Json);

    // In JSON mode, catch errors and emit structured JSON instead of miette output
    let result = process_inner(args, json_mode);

    if json_mode {
        match result {
            Ok(out) => {
                let sha256 = compute_sha256(args.output)?;
                let complete = CompleteEvent {
                    event_type: "complete",
                    input: args.input.display().to_string(),
                    output: args.output.display().to_string(),
                    packets_read: out.packets_read,
                    frames_decoded: out.frames_decoded,
                    frames_encoded: out.frames_encoded,
                    packets_written: out.packets_written,
                    audio_tracks: out.audio_tracks,
                    mux_ok: out.mux_ok,
                    output_codec: out.output_codec,
                    output_duration_secs: out.output_duration_secs,
                    output_bitrate_kbps: out.output_bitrate_kbps,
                    output_sha256: sha256,
                };
                println!("{}", serde_json::to_string(&complete).unwrap());
                Ok(())
            }
            Err(e) => {
                let (error_kind, code) = classify_error(&e);
                let error_json = ErrorResult {
                    event_type: "error",
                    error_kind: error_kind.to_string(),
                    message: format!("{e}"),
                };
                println!("{}", serde_json::to_string(&error_json).unwrap());
                std::process::exit(code);
            }
        }
    } else {
        match result {
            Ok(out) => {
                let sha256 = compute_sha256(args.output)?;
                print_text_summary(args, &out, &sha256);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

/// Resolves the target audio codec from `--audio-codec` or falls back to the
/// container default (MP4 -> AAC, WebM/MKV -> Opus).
fn resolve_target_audio_codec(
    audio_codec_arg: Option<&AudioCodecArg>,
    container: splica_core::ContainerFormat,
) -> splica_core::AudioCodec {
    match audio_codec_arg {
        Some(AudioCodecArg::Aac) => splica_core::AudioCodec::Aac,
        Some(AudioCodecArg::Opus) => splica_core::AudioCodec::Opus,
        None => match container {
            splica_core::ContainerFormat::WebM | splica_core::ContainerFormat::Mkv => {
                splica_core::AudioCodec::Opus
            }
            splica_core::ContainerFormat::Mp4 => splica_core::AudioCodec::Aac,
        },
    }
}

/// Pre-flight check: reject audio codec / container combinations that cannot work.
fn validate_audio_codec_container(
    audio_codec: Option<&AudioCodecArg>,
    container: splica_core::ContainerFormat,
) -> Result<()> {
    if let (Some(AudioCodecArg::Opus), splica_core::ContainerFormat::Mp4) = (audio_codec, container)
    {
        return Err(miette::miette!(
            "Opus is not supported in MP4 — use --audio-codec aac or choose a WebM/MKV output"
        ));
    }
    Ok(())
}

fn process_inner(args: &ProcessArgs<'_>, json_mode: bool) -> Result<TranscodeOutput> {
    validate_output_format(args.output)?;

    let out_container = output_container(args.output).unwrap_or(splica_core::ContainerFormat::Mp4);

    // Validate audio codec / container compatibility before doing any work.
    validate_audio_codec_container(args.audio_codec, out_container)?;

    let start = Instant::now();

    // Determine if user explicitly requested re-encoding via any encoding option
    let user_requested_reencode = args.bitrate.is_some()
        || args.crf.is_some()
        || args.preset.is_some()
        || args.max_fps.is_some()
        || args.resize.is_some()
        || args.crop.is_some()
        || args.volume.is_some()
        || args.codec.is_some()
        || args.audio_bitrate.is_some()
        || args.h264_profile.is_some()
        || args.h264_level.is_some();

    if !user_requested_reencode {
        // Check if stream copy is possible (all codecs compatible, no audio transcode needed)
        let demuxer = super::open_demuxer(args.input)?;
        let tracks = demuxer.tracks().to_vec();

        let video_needs_reencode = tracks
            .iter()
            .filter(|t| t.kind == splica_core::TrackKind::Video)
            .any(|t| !reencode::is_video_codec_compatible(&t.codec, out_container));

        let target_audio_codec = resolve_target_audio_codec(args.audio_codec, out_container);

        let any_audio_needs_transcode = tracks
            .iter()
            .filter(|t| t.kind == splica_core::TrackKind::Audio)
            .any(|t| {
                if let splica_core::Codec::Audio(ref ac) = t.codec {
                    *ac != target_audio_codec
                } else {
                    false
                }
            });

        if !video_needs_reencode && !any_audio_needs_transcode {
            let mut out = stream_copy::stream_copy(args, json_mode)?;
            out.elapsed_secs = start.elapsed().as_secs_f64();
            return Ok(out);
        }
    }

    let mut out = reencode::reencode(args, json_mode)?;
    out.elapsed_secs = start.elapsed().as_secs_f64();
    Ok(out)
}

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

fn format_duration(secs: f64) -> String {
    let total = secs as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    let frac = secs - secs.floor();

    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}.{:01}", (frac * 10.0) as u64)
    }
}

fn print_text_summary(args: &ProcessArgs<'_>, out: &TranscodeOutput, sha256: &str) {
    let input_size = std::fs::metadata(args.input).map(|m| m.len()).unwrap_or(0);
    let output_size = std::fs::metadata(args.output).map(|m| m.len()).unwrap_or(0);

    let mode = if out.is_stream_copy {
        "stream copy".to_string()
    } else {
        match &out.output_codec {
            Some(codec) => format!("re-encode ({codec})"),
            None => "re-encode".to_string(),
        }
    };

    let size_change = if input_size > 0 {
        let pct = ((output_size as f64 - input_size as f64) / input_size as f64) * 100.0;
        if pct >= 0.0 {
            format!("+{pct:.0}%")
        } else {
            format!("{pct:.0}%")
        }
    } else {
        String::new()
    };

    let duration = out
        .output_duration_secs
        .map(format_duration)
        .unwrap_or_else(|| "unknown".to_string());

    let bitrate = out
        .output_bitrate_kbps
        .map(|b| format!("{b} kbps"))
        .unwrap_or_else(|| "unknown".to_string());

    eprintln!();
    eprintln!(
        "  Output: {} -> {} ({} -> {}, {})",
        args.input.display(),
        args.output.display(),
        format_file_size(input_size),
        format_file_size(output_size),
        size_change,
    );
    eprintln!("  Mode: {mode} | Duration: {duration} | Bitrate: {bitrate}");

    if !out.audio_tracks.is_empty() {
        let audio_summary: Vec<String> = out
            .audio_tracks
            .iter()
            .map(|a| {
                let mode_str = match a.mode {
                    AudioMode::PassThrough => "pass-through",
                    AudioMode::ReEncoded => "re-encoded",
                };
                format!("{} ({})", a.codec, mode_str)
            })
            .collect();
        eprintln!("  Audio: {}", audio_summary.join(", "));
    }

    eprintln!("  SHA-256: {sha256}");
    eprintln!("  Time: {:.1}s", out.elapsed_secs);
}
