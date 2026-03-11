use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use miette::{Context, IntoDiagnostic, Result};
use serde::Serialize;

use splica_codec::{H264Decoder, H264EncoderBuilder};
use splica_core::{Codec, Demuxer, Muxer, TrackIndex, TrackKind, VideoCodec};
use splica_filter::{AspectMode, ScaleFilter};
use splica_mp4::boxes::stsd::CodecConfig;
use splica_mp4::{Mp4Demuxer, Mp4Muxer};
use splica_pipeline::{PipelineBuilder, PipelineEventKind};
use splica_webm::{WebmDemuxer, WebmMuxer};

#[derive(Parser)]
#[command(name = "splica", version, about = "Media processing tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Copy streams from input to output without re-encoding.
    Convert {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Inspect a media file and print track information.
    Probe {
        /// Input file path.
        file: PathBuf,

        /// Output format.
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// Extract a time range from a media file (stream copy, no re-encoding).
    Trim {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Start time (e.g., "1:30", "90", "0:01:30.5").
        #[arg(long)]
        start: Option<String>,

        /// End time (e.g., "2:00", "120", "0:02:00").
        #[arg(long)]
        end: Option<String>,
    },

    /// Extract only audio tracks from a media file.
    ExtractAudio {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Transcode video using the pipeline (demux → decode → encode → mux).
    Transcode {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Target video bitrate (e.g., "2M", "1500k", or raw bps).
        #[arg(long)]
        bitrate: Option<String>,

        /// Encoding speed/quality preset.
        #[arg(long, default_value = "medium")]
        preset: EncodePreset,

        /// Maximum frame rate hint for the encoder (e.g., 30, 60).
        #[arg(long)]
        max_fps: Option<f32>,

        /// Resize video to WxH (e.g., "1280x720", "640x480").
        #[arg(long)]
        resize: Option<String>,

        /// Aspect ratio handling when resizing (default: fit).
        #[arg(long, default_value = "fit")]
        aspect_mode: AspectModeArg,

        /// Output format for results (text or json).
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },
}

/// Encoding speed/quality tradeoff preset.
#[derive(Clone, ValueEnum)]
enum EncodePreset {
    /// Fastest encoding, lower quality. Good for previews.
    Fast,
    /// Balanced speed and quality. Good default.
    Medium,
    /// Slower encoding, better quality. Good for final output.
    Slow,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

/// CLI argument for aspect ratio handling.
#[derive(Clone, ValueEnum)]
enum AspectModeArg {
    /// Stretch to fill target (may distort).
    Stretch,
    /// Fit within target, adding black bars.
    Fit,
    /// Fill target, cropping excess.
    Fill,
}

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

/// Structured exit codes for automation.
#[allow(dead_code)]
mod exit_code {
    /// Success.
    pub const SUCCESS: i32 = 0;
    /// Bad input: malformed file, unsupported format, invalid arguments. Do not retry.
    pub const BAD_INPUT: i32 = 1;
    /// Internal error: encoder/muxer failure. May retry.
    pub const INTERNAL: i32 = 2;
}

// ---------------------------------------------------------------------------
// Transcode JSON output
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct TranscodeResult {
    status: String,
    input: String,
    output: String,
    packets_read: u64,
    frames_decoded: u64,
    frames_encoded: u64,
    packets_written: u64,
    audio_tracks: Vec<TranscodeAudioInfo>,
}

#[derive(Serialize)]
struct TranscodeAudioInfo {
    codec: String,
    sample_rate: u32,
    channels: Option<u32>,
    mode: String, // "pass_through" or "re_encoded"
}

#[derive(Serialize)]
struct ErrorResult {
    status: String,
    error_kind: String,
    message: String,
}

/// Classifies an error into an error_kind string and exit code.
///
/// Returns `(error_kind, exit_code)` based on the error message content.
fn classify_error(error: &miette::Report) -> (&'static str, i32) {
    let msg = format!("{error:?}");

    // I/O errors (file not found, permission denied, etc.) — bad input
    if msg.contains("could not open file")
        || msg.contains("could not create output file")
        || msg.contains("unsupported output format")
        || msg.contains("unsupported container format")
        || msg.contains("no extension")
        || msg.contains("no H.264 video tracks")
        || msg.contains("invalid resize")
        || msg.contains("invalid bitrate")
        || msg.contains("invalid time")
    {
        return ("bad_input", exit_code::BAD_INPUT);
    }

    // Encoder/decoder/pipeline internal failures — may be retryable
    if msg.contains("transcode failed")
        || msg.contains("encode failed")
        || msg.contains("decode failed")
        || msg.contains("failed to build transcode pipeline")
    {
        return ("internal_error", exit_code::INTERNAL);
    }

    // Default to bad_input for unrecognized errors
    ("bad_input", exit_code::BAD_INPUT)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Convert { input, output } => convert(&input, &output),
        Commands::Probe { file, format } => probe(&file, &format),
        Commands::Trim {
            input,
            output,
            start,
            end,
        } => trim(&input, &output, start.as_deref(), end.as_deref()),
        Commands::ExtractAudio { input, output } => extract_audio(&input, &output),
        Commands::Transcode {
            input,
            output,
            bitrate,
            preset,
            max_fps,
            resize,
            aspect_mode,
            format,
        } => transcode(
            &TranscodeArgs {
                input: &input,
                output: &output,
                bitrate: bitrate.as_deref(),
                preset: &preset,
                max_fps,
                resize: resize.as_deref(),
                aspect_mode_arg: &aspect_mode,
            },
            &format,
        ),
    }
}

// ---------------------------------------------------------------------------
// Output format validation
// ---------------------------------------------------------------------------

/// Validates that the output file extension is a format splica can write.
/// Fails fast before any input is read, with a helpful error message.
fn validate_output_format(output: &Path) -> Result<()> {
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "mp4" | "m4v" | "m4a" | "webm" => Ok(()),
        "mkv" | "mka" => Err(miette::miette!(
            "output format 'mkv' is not yet supported for writing\n  \
             → splica can currently write: mp4\n  \
             → MKV output is planned for a future release"
        )),
        "" => Err(miette::miette!(
            "output file has no extension — cannot determine output format\n  \
             → Use a recognized extension: .mp4"
        )),
        other => Err(miette::miette!(
            "unsupported output format '.{other}'\n  \
             → splica can currently write: mp4\n  \
             → Supported extensions: .mp4, .m4v, .m4a"
        )),
    }
}

// ---------------------------------------------------------------------------
// Time parsing
// ---------------------------------------------------------------------------

/// Parses a human-readable timestamp into seconds.
///
/// Supported formats:
/// - "90" or "90.5" — seconds
/// - "1:30" or "1:30.5" — minutes:seconds
/// - "0:01:30" or "0:01:30.5" — hours:minutes:seconds
fn parse_time(s: &str) -> Result<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        1 => {
            // seconds only
            parts[0]
                .parse::<f64>()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))
        }
        2 => {
            // minutes:seconds
            let minutes: f64 = parts[0]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            let seconds: f64 = parts[1]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            Ok(minutes * 60.0 + seconds)
        }
        3 => {
            // hours:minutes:seconds
            let hours: f64 = parts[0]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            let minutes: f64 = parts[1]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            let seconds: f64 = parts[2]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            Ok(hours * 3600.0 + minutes * 60.0 + seconds)
        }
        _ => Err(miette::miette!(
            "invalid time format: '{s}' — use seconds, M:SS, or H:MM:SS"
        )),
    }
}

// ---------------------------------------------------------------------------
// Format detection
// ---------------------------------------------------------------------------

/// Detected container format.
enum ContainerFormat {
    Mp4,
    WebM,
}

/// Sniffs the container format from the first few bytes of a file.
fn detect_format(file: &mut (impl Read + Seek)) -> Result<ContainerFormat> {
    let mut magic = [0u8; 12];
    let bytes_read = file
        .read(&mut magic)
        .into_diagnostic()
        .wrap_err("could not read file header")?;
    file.seek(SeekFrom::Start(0))
        .into_diagnostic()
        .wrap_err("could not seek back to start")?;

    if bytes_read < 4 {
        return Err(miette::miette!(
            "file too small to detect format ({bytes_read} bytes)"
        ));
    }

    // WebM/Matroska: EBML header starts with 0x1A 0x45 0xDF 0xA3
    if magic[0] == 0x1A && magic[1] == 0x45 && magic[2] == 0xDF && magic[3] == 0xA3 {
        return Ok(ContainerFormat::WebM);
    }

    // MP4: "ftyp" box at bytes 4-7
    if bytes_read >= 8 && &magic[4..8] == b"ftyp" {
        return Ok(ContainerFormat::Mp4);
    }

    Err(miette::miette!(
        "unsupported container format — splica supports MP4 and WebM"
    ))
}

/// Returns true if the output path has a WebM extension.
fn is_webm_output(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase() == "webm")
        .unwrap_or(false)
}

/// Opens a demuxer for the given file, auto-detecting the container format.
fn open_demuxer(path: &Path) -> Result<Box<dyn Demuxer>> {
    let mut file = File::open(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open file '{}'", path.display()))?;

    let format = detect_format(&mut file)?;

    match format {
        ContainerFormat::Mp4 => {
            let demuxer = Mp4Demuxer::open(file)
                .into_diagnostic()
                .wrap_err("failed to parse MP4 container")?;
            Ok(Box::new(demuxer))
        }
        ContainerFormat::WebM => {
            let demuxer = WebmDemuxer::open(BufReader::new(file))
                .into_diagnostic()
                .wrap_err("failed to parse WebM container")?;
            Ok(Box::new(demuxer))
        }
    }
}

/// Creates an output muxer based on file extension.
fn create_muxer(output: &Path) -> Result<Box<dyn Muxer>> {
    let out_file = File::create(output)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not create output file '{}'", output.display()))?;

    if is_webm_output(output) {
        Ok(Box::new(WebmMuxer::new(BufWriter::new(out_file))))
    } else {
        Ok(Box::new(Mp4Muxer::new(BufWriter::new(out_file))))
    }
}

// ---------------------------------------------------------------------------
// probe
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
    sample_rate: Option<u32>,
    channels: Option<u32>,
    channel_layout: Option<String>,
}

fn probe(file: &Path, format: &OutputFormat) -> Result<()> {
    let demuxer = open_demuxer(file)?;

    let tracks: Vec<ProbeTrack> = demuxer
        .tracks()
        .iter()
        .map(|t| {
            let codec = format_codec(&t.codec);

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

fn format_codec(codec: &splica_core::Codec) -> String {
    match codec {
        splica_core::Codec::Video(vc) => match vc {
            splica_core::VideoCodec::H264 => "H.264".to_string(),
            splica_core::VideoCodec::H265 => "H.265".to_string(),
            splica_core::VideoCodec::Av1 => "AV1".to_string(),
            splica_core::VideoCodec::Other(s) => s.clone(),
        },
        splica_core::Codec::Audio(ac) => match ac {
            splica_core::AudioCodec::Aac => "AAC".to_string(),
            splica_core::AudioCodec::Opus => "Opus".to_string(),
            splica_core::AudioCodec::Other(s) => s.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// convert
// ---------------------------------------------------------------------------

fn convert(input: &Path, output: &Path) -> Result<()> {
    validate_output_format(output)?;
    let mut demuxer = open_demuxer(input)?;

    let out_file = File::create(output)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not create output file '{}'", output.display()))?;

    let mut muxer: Box<dyn Muxer> = if is_webm_output(output) {
        Box::new(WebmMuxer::new(BufWriter::new(out_file)))
    } else {
        Box::new(Mp4Muxer::new(BufWriter::new(out_file)))
    };

    let track_count = demuxer.tracks().len();
    for i in 0..track_count {
        let info = demuxer.tracks()[i].clone();
        muxer
            .add_track(&info)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to add track {i}"))?;
    }

    let mut packet_count: u64 = 0;
    while let Some(packet) = demuxer
        .read_packet()
        .into_diagnostic()
        .wrap_err("failed to read packet")?
    {
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

    eprintln!(
        "Copied {packet_count} packets across {track_count} tracks to {}",
        output.display()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// trim
// ---------------------------------------------------------------------------

fn trim(input: &Path, output: &Path, start: Option<&str>, end: Option<&str>) -> Result<()> {
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
                eprintln!("  Trimmed from {actual:.2}s (snapped from {s:.2}s to nearest keyframe)");
            } else {
                eprintln!("  Start: {s:.2}s");
            }
        } else {
            eprintln!("  Start: {s:.2}s");
        }
    } else if let Some(e) = end_secs {
        eprintln!("  End: {e:.2}s");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// extract-audio
// ---------------------------------------------------------------------------

fn extract_audio(input: &Path, output: &Path) -> Result<()> {
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
    let mut input_to_output: std::collections::HashMap<u32, TrackIndex> =
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

// ---------------------------------------------------------------------------
// transcode
// ---------------------------------------------------------------------------

/// Parses a bitrate string like "2M", "1500k", or "1000000" into bits per second.
fn parse_bitrate(s: &str) -> Result<u32> {
    let s = s.trim();
    if let Some(prefix) = s.strip_suffix('M') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000_000.0) as u32)
    } else if let Some(prefix) = s.strip_suffix('m') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000_000.0) as u32)
    } else if let Some(prefix) = s.strip_suffix('k') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000.0) as u32)
    } else if let Some(prefix) = s.strip_suffix('K') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000.0) as u32)
    } else {
        s.parse::<u32>().into_diagnostic().wrap_err_with(|| {
            format!("invalid bitrate: '{s}' — use e.g. '2M', '1500k', or raw bps")
        })
    }
}

/// Parses a "WxH" resize string into (width, height).
fn parse_resize(s: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = s.split('x').collect();
    if parts.len() != 2 {
        return Err(miette::miette!(
            "invalid resize format: '{s}' — use WxH (e.g., '1280x720')"
        ));
    }
    let w: u32 = parts[0]
        .parse()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid width in resize: '{s}'"))?;
    let h: u32 = parts[1]
        .parse()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid height in resize: '{s}'"))?;
    if w == 0 || h == 0 {
        return Err(miette::miette!("resize dimensions must be non-zero: '{s}'"));
    }
    Ok((w, h))
}

struct TranscodeArgs<'a> {
    input: &'a Path,
    output: &'a Path,
    bitrate: Option<&'a str>,
    preset: &'a EncodePreset,
    max_fps: Option<f32>,
    resize: Option<&'a str>,
    aspect_mode_arg: &'a AspectModeArg,
}

fn transcode(args: &TranscodeArgs<'_>, format: &OutputFormat) -> Result<()> {
    let json_mode = matches!(format, OutputFormat::Json);

    // In JSON mode, catch errors and emit structured JSON instead of miette output
    let result = transcode_inner(args, json_mode);

    if json_mode {
        match result {
            Ok((packets_read, frames_decoded, frames_encoded, packets_written, audio_tracks)) => {
                let output_json = TranscodeResult {
                    status: "ok".to_string(),
                    input: args.input.display().to_string(),
                    output: args.output.display().to_string(),
                    packets_read,
                    frames_decoded,
                    frames_encoded,
                    packets_written,
                    audio_tracks,
                };
                println!("{}", serde_json::to_string_pretty(&output_json).unwrap());
                Ok(())
            }
            Err(e) => {
                let (error_kind, code) = classify_error(&e);
                let error_json = ErrorResult {
                    status: "error".to_string(),
                    error_kind: error_kind.to_string(),
                    message: format!("{e}"),
                };
                println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
                std::process::exit(code);
            }
        }
    } else {
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

type TranscodeOutput = (u64, u64, u64, u64, Vec<TranscodeAudioInfo>);
type DemuxerWithConfigs = (Box<dyn Demuxer>, Vec<(TrackIndex, Vec<u8>)>);

fn transcode_inner(args: &TranscodeArgs<'_>, json_mode: bool) -> Result<TranscodeOutput> {
    validate_output_format(args.output)?;
    let bitrate_bps = match args.bitrate {
        Some(s) => parse_bitrate(s)?,
        None => match args.preset {
            EncodePreset::Fast => 500_000,     // 500 kbps
            EncodePreset::Medium => 1_000_000, // 1 Mbps
            EncodePreset::Slow => 2_000_000,   // 2 Mbps
        },
    };

    let frame_rate_hint = args.max_fps.unwrap_or(match args.preset {
        EncodePreset::Fast => 30.0,
        EncodePreset::Medium => 30.0,
        EncodePreset::Slow => 60.0,
    });

    // Try MP4 first (for codec config access), fall back to generic demuxer
    let mut file = File::open(args.input)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open file '{}'", args.input.display()))?;
    let format = detect_format(&mut file)?;

    let (demuxer, video_track_configs): DemuxerWithConfigs = match format {
        ContainerFormat::Mp4 => {
            let mp4 = Mp4Demuxer::open(file)
                .into_diagnostic()
                .wrap_err("failed to parse MP4 container")?;
            let tracks = mp4.tracks().to_vec();
            let mut configs = Vec::new();
            for track in &tracks {
                if track.kind == TrackKind::Video {
                    if let Codec::Video(VideoCodec::H264) = &track.codec {
                        if let Some(CodecConfig::Avc1 { avcc, .. }) = mp4.codec_config(track.index)
                        {
                            configs.push((track.index, avcc.to_vec()));
                        }
                    }
                }
            }
            (Box::new(mp4), configs)
        }
        ContainerFormat::WebM => {
            let webm = WebmDemuxer::open(BufReader::new(file))
                .into_diagnostic()
                .wrap_err("failed to parse WebM container")?;
            // WebM doesn't expose MP4-style codec config; H.264 in WebM is unsupported
            (Box::new(webm), Vec::new())
        }
    };

    let tracks = demuxer.tracks().to_vec();

    // Collect audio track metadata for JSON output
    let audio_tracks: Vec<TranscodeAudioInfo> = tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .map(|t| {
            let codec = format_codec(&t.codec);
            let sample_rate = t.audio.as_ref().map(|a| a.sample_rate).unwrap_or(0);
            let channels = t
                .audio
                .as_ref()
                .and_then(|a| a.channel_layout.map(|cl| cl.channel_count()));
            TranscodeAudioInfo {
                codec,
                sample_rate,
                channels,
                mode: "pass_through".to_string(),
            }
        })
        .collect();

    if video_track_configs.is_empty() {
        return Err(miette::miette!(
            "no H.264 video tracks found in '{}' — transcode currently supports H.264 only",
            args.input.display()
        ));
    }

    let muxer = create_muxer(args.output)?;

    // Shared counters for JSON output
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let counter_packets_read = Arc::new(AtomicU64::new(0));
    let counter_frames_decoded = Arc::new(AtomicU64::new(0));
    let counter_frames_encoded = Arc::new(AtomicU64::new(0));
    let counter_packets_written = Arc::new(AtomicU64::new(0));

    let pr = Arc::clone(&counter_packets_read);
    let fd = Arc::clone(&counter_frames_decoded);
    let fe = Arc::clone(&counter_frames_encoded);
    let pw = Arc::clone(&counter_packets_written);

    // Build pipeline: video tracks get decoder+encoder, audio tracks copy through
    let mut builder = PipelineBuilder::new()
        .with_event_handler(move |event| match event.kind {
            PipelineEventKind::PacketsRead { count } => {
                pr.store(count, Ordering::Relaxed);
                if !json_mode && count % 100 == 0 {
                    eprint!("\r  Packets read: {count}");
                }
            }
            PipelineEventKind::FramesDecoded { count } => {
                fd.store(count, Ordering::Relaxed);
                if !json_mode && count % 100 == 0 {
                    eprint!("  Decoded: {count}");
                }
            }
            PipelineEventKind::FramesEncoded { count } => {
                fe.store(count, Ordering::Relaxed);
                if !json_mode && count % 100 == 0 {
                    eprint!("  Encoded: {count}");
                }
            }
            PipelineEventKind::PacketsWritten { count } => {
                pw.store(count, Ordering::Relaxed);
            }
            _ => {}
        })
        .with_demuxer(demuxer)
        .with_muxer(muxer);

    for (track_idx, avcc_data) in &video_track_configs {
        let decoder = H264Decoder::new(avcc_data)
            .into_diagnostic()
            .wrap_err("failed to create H.264 decoder")?;

        let encoder = H264EncoderBuilder::new()
            .bitrate(bitrate_bps)
            .max_frame_rate(frame_rate_hint)
            .track_index(*track_idx)
            .build()
            .into_diagnostic()
            .wrap_err("failed to create H.264 encoder")?;

        builder = builder.with_decoder(*track_idx, decoder);
        builder = builder.with_encoder(*track_idx, encoder);

        // Add scale filter if --resize was specified
        if let Some(resize_str) = args.resize {
            let (w, h) = parse_resize(resize_str)?;
            let aspect_mode = match args.aspect_mode_arg {
                AspectModeArg::Stretch => AspectMode::Stretch,
                AspectModeArg::Fit => AspectMode::Fit,
                AspectModeArg::Fill => AspectMode::Fill,
            };
            let scale_filter = ScaleFilter::new(w, h).with_aspect_mode(aspect_mode);
            builder = builder.with_filter(*track_idx, scale_filter);
        }
    }

    let mut pipeline = builder
        .build()
        .into_diagnostic()
        .wrap_err("failed to build transcode pipeline")?;

    let preset_name = match args.preset {
        EncodePreset::Fast => "fast",
        EncodePreset::Medium => "medium",
        EncodePreset::Slow => "slow",
    };
    if let Some(resize_str) = args.resize {
        eprintln!(
            "Transcoding {} → {} (H.264, {} kbps, preset: {preset_name}, resize: {resize_str})",
            args.input.display(),
            args.output.display(),
            bitrate_bps / 1000
        );
    } else {
        eprintln!(
            "Transcoding {} → {} (H.264, {} kbps, preset: {preset_name})",
            args.input.display(),
            args.output.display(),
            bitrate_bps / 1000
        );
    }

    pipeline
        .run()
        .into_diagnostic()
        .wrap_err("transcode failed")?;

    if !json_mode {
        eprintln!("\r  Done.                                        ");
    }

    Ok((
        counter_packets_read.load(Ordering::Relaxed),
        counter_frames_decoded.load(Ordering::Relaxed),
        counter_frames_encoded.load(Ordering::Relaxed),
        counter_packets_written.load(Ordering::Relaxed),
        audio_tracks,
    ))
}
