pub(crate) mod extract_audio;
pub(crate) mod migrate;
pub(crate) mod probe;
pub(crate) mod process;
pub(crate) mod trim;

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom};
use std::path::Path;

use clap::ValueEnum;
use miette::{Context, IntoDiagnostic, Result};
use serde::Serialize;

use splica_core::{
    ContainerFormat, DecodeError, DemuxError, Demuxer, EncodeError, ErrorKind, FilterError,
    MuxError, Muxer, PipelineError,
};
use splica_mkv::MkvMuxer;
use splica_mp4::{Mp4Demuxer, Mp4Muxer};
use splica_webm::{WebmDemuxer, WebmMuxer};

// ---------------------------------------------------------------------------
// Shared CLI enums
// ---------------------------------------------------------------------------

#[derive(Clone, ValueEnum)]
pub(crate) enum OutputFormat {
    Text,
    Json,
}

/// Encoding speed/quality tradeoff preset.
#[derive(Clone, ValueEnum)]
pub(crate) enum EncodePreset {
    /// Fastest encoding, lower quality. Good for previews.
    Fast,
    /// Balanced speed and quality. Good default.
    Medium,
    /// Slower encoding, better quality. Good for final output.
    Slow,
}

/// CLI argument for output video codec selection.
#[derive(Clone, ValueEnum)]
pub(crate) enum VideoCodecArg {
    /// H.264 / AVC (default for MP4 and MKV).
    H264,
    /// H.265 / HEVC (requires kvazaar).
    H265,
}

/// CLI argument for aspect ratio handling.
#[derive(Clone, ValueEnum)]
pub(crate) enum AspectModeArg {
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
pub(crate) mod exit_code {
    /// Success.
    #[allow(dead_code)]
    pub const SUCCESS: i32 = 0;
    /// Bad input: malformed file, unsupported format, invalid arguments. Do not retry.
    pub const BAD_INPUT: i32 = 1;
    /// Internal error: encoder/muxer failure. May retry.
    pub const INTERNAL: i32 = 2;
    /// Resource exhaustion: memory, file handles, budget limits. Retry after backoff.
    pub const RESOURCE_EXHAUSTED: i32 = 3;
}

// ---------------------------------------------------------------------------
// JSON output types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct CompleteEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub input: String,
    pub output: String,
    pub packets_read: u64,
    pub frames_decoded: u64,
    pub frames_encoded: u64,
    pub packets_written: u64,
    pub audio_tracks: Vec<TranscodeAudioInfo>,
    pub mux_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_duration_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_bitrate_kbps: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct TranscodeAudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: Option<u32>,
    pub mode: AudioMode,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AudioMode {
    PassThrough,
    ReEncoded,
}

#[derive(Serialize)]
pub(crate) struct ProgressEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub packets_read: u64,
    pub frames_decoded: u64,
    pub frames_encoded: u64,
    pub packets_written: u64,
}

#[derive(Serialize)]
pub(crate) struct ErrorResult {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub error_kind: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

/// Classifies an error into an error_kind string and exit code.
///
/// Walks the error source chain to find a typed splica error with an
/// `ErrorKind`. This is robust against error message changes — classification
/// depends on error variants, not string content.
pub(crate) fn classify_error(error: &miette::Report) -> (&'static str, i32) {
    if let Some(kind) = extract_error_kind(error) {
        return error_kind_to_classification(kind);
    }
    // Default: unrecognized errors are bad input (user-facing CLI errors
    // like invalid arguments don't wrap splica library errors).
    ("bad_input", exit_code::BAD_INPUT)
}

/// Walks the error source chain looking for a typed splica error.
fn extract_error_kind(error: &miette::Report) -> Option<ErrorKind> {
    let mut current: &dyn std::error::Error = error.as_ref();
    loop {
        if let Some(e) = current.downcast_ref::<PipelineError>() {
            return Some(e.kind());
        }
        if let Some(e) = current.downcast_ref::<DemuxError>() {
            return Some(e.kind());
        }
        if let Some(e) = current.downcast_ref::<DecodeError>() {
            return Some(e.kind());
        }
        if let Some(e) = current.downcast_ref::<EncodeError>() {
            return Some(e.kind());
        }
        if let Some(e) = current.downcast_ref::<MuxError>() {
            return Some(e.kind());
        }
        if let Some(e) = current.downcast_ref::<FilterError>() {
            return Some(e.kind());
        }
        match current.source() {
            Some(next) => current = next,
            None => return None,
        }
    }
}

fn error_kind_to_classification(kind: ErrorKind) -> (&'static str, i32) {
    match kind {
        ErrorKind::InvalidInput => ("bad_input", exit_code::BAD_INPUT),
        ErrorKind::UnsupportedFormat => ("unsupported_format", exit_code::BAD_INPUT),
        ErrorKind::Io => ("internal_error", exit_code::INTERNAL),
        ErrorKind::ResourceExhausted => ("resource_exhausted", exit_code::RESOURCE_EXHAUSTED),
        ErrorKind::Internal => ("internal_error", exit_code::INTERNAL),
    }
}

// ---------------------------------------------------------------------------
// Output format validation
// ---------------------------------------------------------------------------

/// Validates that the output file extension is a format splica can write.
/// Fails fast before any input is read, with a helpful error message.
pub(crate) fn validate_output_format(output: &Path) -> Result<()> {
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ContainerFormat::from_extension(ext) {
        Some(fmt) if fmt.is_writable() => Ok(()),
        Some(_) => Ok(()), // unreachable since all recognized formats are writable
        None if ext.is_empty() => Err(miette::miette!(
            "output file has no extension — cannot determine output format\n  \
             → Use a recognized extension: .mp4, .webm, .mkv"
        )),
        None => Err(miette::miette!(
            "unsupported output format '.{ext}'\n  \
             → splica can currently write: mp4, webm, mkv\n  \
             → Supported extensions: .mp4, .m4v, .m4a, .webm, .mkv, .mka"
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
pub(crate) fn parse_time(s: &str) -> Result<f64> {
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

/// Detected input container format (from magic bytes).
pub(crate) enum DetectedFormat {
    Mp4,
    WebM,
}

/// Sniffs the container format from the first few bytes of a file.
pub(crate) fn detect_format(file: &mut (impl Read + Seek)) -> Result<DetectedFormat> {
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

    // WebM/MKV (Matroska): EBML header starts with 0x1A 0x45 0xDF 0xA3
    if magic[0] == 0x1A && magic[1] == 0x45 && magic[2] == 0xDF && magic[3] == 0xA3 {
        return Ok(DetectedFormat::WebM);
    }

    // MP4: "ftyp" box at bytes 4-7
    if bytes_read >= 8 && &magic[4..8] == b"ftyp" {
        return Ok(DetectedFormat::Mp4);
    }

    Err(miette::miette!(
        "unsupported container format — splica supports MP4, WebM, and MKV"
    ))
}

// ---------------------------------------------------------------------------
// Container helpers
// ---------------------------------------------------------------------------

pub(crate) fn output_container(path: &Path) -> Option<ContainerFormat> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    ContainerFormat::from_extension(ext)
}

/// Opens a demuxer for the given file, auto-detecting the container format.
pub(crate) fn open_demuxer(path: &Path) -> Result<Box<dyn Demuxer>> {
    let mut file = File::open(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open file '{}'", path.display()))?;

    let format = detect_format(&mut file)?;

    match format {
        DetectedFormat::Mp4 => {
            let demuxer = Mp4Demuxer::open(file)
                .into_diagnostic()
                .wrap_err("failed to parse MP4 container")?;
            Ok(Box::new(demuxer))
        }
        DetectedFormat::WebM => {
            let demuxer = WebmDemuxer::open(BufReader::new(file))
                .into_diagnostic()
                .wrap_err("failed to parse WebM container")?;
            Ok(Box::new(demuxer))
        }
    }
}

/// Creates an output muxer based on file extension.
pub(crate) fn create_muxer(output: &Path) -> Result<Box<dyn Muxer>> {
    let out_file = File::create(output)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not create output file '{}'", output.display()))?;

    match output_container(output) {
        Some(ContainerFormat::WebM) => Ok(Box::new(WebmMuxer::new(BufWriter::new(out_file)))),
        Some(ContainerFormat::Mkv) => Ok(Box::new(MkvMuxer::new(BufWriter::new(out_file)))),
        _ => Ok(Box::new(Mp4Muxer::new(BufWriter::new(out_file)))),
    }
}
