pub(crate) mod extract_audio;
pub(crate) mod factories;
pub(crate) mod format_detect;
pub(crate) mod join;
pub(crate) mod migrate;
pub(crate) mod probe;
pub(crate) mod process;
pub(crate) mod time;
pub(crate) mod trim;

use clap::ValueEnum;
use serde::Serialize;

use splica_core::{
    ContainerFormat, DecodeError, DemuxError, EncodeError, ErrorKind, FilterError, MuxError,
    PipelineError,
};

pub(crate) use factories::{create_muxer, open_demuxer, output_container, validate_output_format};
pub(crate) use format_detect::{detect_format, DetectedFormat};
pub(crate) use time::parse_time;

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
    /// AV1 (default for WebM, also valid in MP4 and MKV).
    Av1,
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
