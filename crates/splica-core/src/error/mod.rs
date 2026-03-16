//! Error types and error hierarchy for the splica media processing library.
//!
//! Each pipeline stage has its own error type with domain-relevant variants.
//! All error types expose an [`ErrorKind`] for automated retry/abort decisions.

use std::fmt;

use crate::media::TrackIndex;

#[cfg(feature = "serde")]
mod serde_impls;

/// Broad categorization of errors for automated retry/abort decisions.
///
/// Platform engineers (like Priya) match on this to decide whether to retry
/// a failed operation or abort immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ErrorKind {
    /// Input is malformed or invalid — retrying won't help.
    InvalidInput,
    /// Format or codec is recognized but not supported — retrying won't help.
    UnsupportedFormat,
    /// I/O error — may be transient, retry may succeed.
    Io,
    /// Resource limit exceeded (memory, file handles) — retry after backoff may help.
    ResourceExhausted,
    /// Bug in splica — should be reported.
    Internal,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput => write!(f, "invalid input"),
            Self::UnsupportedFormat => write!(f, "unsupported format"),
            Self::Io => write!(f, "I/O error"),
            Self::ResourceExhausted => write!(f, "resource exhausted"),
            Self::Internal => write!(f, "internal error"),
        }
    }
}

impl ErrorKind {
    /// All variants of `ErrorKind`, in declaration order.
    ///
    /// Used by contract tests to ensure the documented error taxonomy stays
    /// in sync with the code. If you add a new variant, add it here too —
    /// the exhaustiveness test will fail if you forget.
    pub const ALL: &[ErrorKind] = &[
        ErrorKind::InvalidInput,
        ErrorKind::UnsupportedFormat,
        ErrorKind::Io,
        ErrorKind::ResourceExhausted,
        ErrorKind::Internal,
    ];

    /// Returns `true` if the error may be transient and retrying could succeed.
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Io | Self::ResourceExhausted)
    }

    /// The stable snake_case string emitted as `error_kind` in JSON error output.
    ///
    /// These strings are part of the stability contract (`docs/exit-codes.md`)
    /// and MUST NOT change across minor versions.
    pub fn as_error_kind_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "bad_input",
            Self::UnsupportedFormat => "unsupported_format",
            Self::Io => "internal_error",
            Self::ResourceExhausted => "resource_exhausted",
            Self::Internal => "internal_error",
        }
    }

    /// The CLI exit code for this error kind.
    pub fn exit_code(self) -> i32 {
        match self {
            Self::InvalidInput | Self::UnsupportedFormat => 1,
            Self::Io | Self::Internal => 2,
            Self::ResourceExhausted => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// DemuxError
// ---------------------------------------------------------------------------

/// Errors produced by demuxer operations (reading container formats).
#[derive(Debug, thiserror::Error)]
pub enum DemuxError {
    #[error("invalid container at offset {offset}: {message}")]
    InvalidContainer { offset: u64, message: String },

    #[error("unsupported codec '{codec}' — splica v0.1 supports H.264, H.265, and AV1")]
    UnsupportedCodec { codec: String },

    #[error("unexpected end of file at offset {offset}")]
    UnexpectedEof { offset: u64 },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl DemuxError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidContainer { .. } | Self::UnexpectedEof { .. } => ErrorKind::InvalidInput,
            Self::UnsupportedCodec { .. } => ErrorKind::UnsupportedFormat,
            Self::Io(_) => ErrorKind::Io,
            Self::Other(_) => ErrorKind::Internal,
        }
    }
}

// ---------------------------------------------------------------------------
// DecodeError
// ---------------------------------------------------------------------------

/// Errors produced by decoder operations (decompressing packets into frames).
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("invalid bitstream: {message}")]
    InvalidBitstream { message: String },

    #[error("unsupported codec profile '{profile}' for codec '{codec}'")]
    UnsupportedProfile { codec: String, profile: String },

    #[error("decoder resource exhausted: {message}")]
    ResourceExhausted { message: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl DecodeError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidBitstream { .. } => ErrorKind::InvalidInput,
            Self::UnsupportedProfile { .. } => ErrorKind::UnsupportedFormat,
            Self::ResourceExhausted { .. } => ErrorKind::ResourceExhausted,
            Self::Io(_) => ErrorKind::Io,
            Self::Other(_) => ErrorKind::Internal,
        }
    }
}

// ---------------------------------------------------------------------------
// EncodeError
// ---------------------------------------------------------------------------

/// Errors produced by encoder operations (compressing frames into packets).
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("invalid frame: {message}")]
    InvalidFrame { message: String },

    #[error("unsupported encoding configuration: {message}")]
    UnsupportedConfig { message: String },

    #[error("encoder resource exhausted: {message}")]
    ResourceExhausted { message: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl EncodeError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidFrame { .. } => ErrorKind::InvalidInput,
            Self::UnsupportedConfig { .. } => ErrorKind::UnsupportedFormat,
            Self::ResourceExhausted { .. } => ErrorKind::ResourceExhausted,
            Self::Io(_) => ErrorKind::Io,
            Self::Other(_) => ErrorKind::Internal,
        }
    }
}

// ---------------------------------------------------------------------------
// MuxError
// ---------------------------------------------------------------------------

/// Errors produced by muxer operations (writing container formats).
#[derive(Debug, thiserror::Error)]
pub enum MuxError {
    #[error("invalid track configuration: {message}")]
    InvalidTrackConfig { message: String },

    #[error("unsupported codec '{codec}' for container format '{container}'")]
    IncompatibleCodec { codec: String, container: String },

    #[error("muxer resource exhausted: {message}")]
    ResourceExhausted { message: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl MuxError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidTrackConfig { .. } => ErrorKind::InvalidInput,
            Self::IncompatibleCodec { .. } => ErrorKind::UnsupportedFormat,
            Self::ResourceExhausted { .. } => ErrorKind::ResourceExhausted,
            Self::Io(_) => ErrorKind::Io,
            Self::Other(_) => ErrorKind::Internal,
        }
    }
}

// ---------------------------------------------------------------------------
// FilterError
// ---------------------------------------------------------------------------

/// Errors produced by filter operations (transforming frames).
#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("invalid filter input: {message}")]
    InvalidInput { message: String },

    #[error("filter resource exhausted: {message}")]
    ResourceExhausted { message: String },

    #[error("{0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl FilterError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidInput { .. } => ErrorKind::InvalidInput,
            Self::ResourceExhausted { .. } => ErrorKind::ResourceExhausted,
            Self::Other(_) => ErrorKind::Internal,
        }
    }
}

// ---------------------------------------------------------------------------
// ValidationError
// ---------------------------------------------------------------------------

/// Pre-flight validation errors detected before the pipeline runs.
///
/// These errors are configuration mistakes that can be caught statically from
/// the builder state — no input data needs to be read. Calling
/// `PipelineBuilder::validate()` returns all of them at once; `build()` returns
/// the first one wrapped in a `PipelineError::Validation`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("pipeline requires a demuxer")]
    MissingDemuxer,

    #[error("pipeline requires a muxer")]
    MissingMuxer,

    #[error("track {} has an encoder but no decoder — both are required for transcoding", .0.0)]
    EncoderWithoutDecoder(TrackIndex),

    #[error("track {} has a decoder but no encoder — both are required for transcoding", .0.0)]
    DecoderWithoutEncoder(TrackIndex),

    #[error("track {} has an audio encoder but no audio decoder — both are required for transcoding", .0.0)]
    AudioEncoderWithoutDecoder(TrackIndex),

    #[error("track {} has an audio decoder but no audio encoder — both are required for transcoding", .0.0)]
    AudioDecoderWithoutEncoder(TrackIndex),

    #[error("track {} has video filter(s) but no decoder/encoder — filters require transcoding", .0.0)]
    OrphanVideoFilter(TrackIndex),

    #[error("track {} has audio filter(s) but no audio decoder/encoder — filters require transcoding", .0.0)]
    OrphanAudioFilter(TrackIndex),
}

impl ValidationError {
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::InvalidInput
    }
}

// ---------------------------------------------------------------------------
// PipelineError
// ---------------------------------------------------------------------------

/// Errors produced by pipeline orchestration.
///
/// Wraps stage-specific errors with pipeline context.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("demux failed: {0}")]
    Demux(#[from] DemuxError),

    #[error("decode failed: {0}")]
    Decode(#[from] DecodeError),

    #[error("filter failed: {0}")]
    Filter(#[from] FilterError),

    #[error("encode failed: {0}")]
    Encode(#[from] EncodeError),

    #[error("mux failed: {0}")]
    Mux(#[from] MuxError),

    #[error("pipeline configuration error: {message}")]
    Config { message: String },

    #[error("pipeline validation failed: {0}")]
    Validation(#[from] ValidationError),

    #[error("{0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl PipelineError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Demux(e) => e.kind(),
            Self::Decode(e) => e.kind(),
            Self::Filter(e) => e.kind(),
            Self::Encode(e) => e.kind(),
            Self::Mux(e) => e.kind(),
            Self::Config { .. } => ErrorKind::InvalidInput,
            Self::Validation(e) => e.kind(),
            Self::Other(_) => ErrorKind::Internal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_error_kind_all_is_exhaustive() {
        // This match has no wildcard. Adding a new ErrorKind variant
        // without updating ALL will cause a compile error here.
        for &kind in ErrorKind::ALL {
            match kind {
                ErrorKind::InvalidInput
                | ErrorKind::UnsupportedFormat
                | ErrorKind::Io
                | ErrorKind::ResourceExhausted
                | ErrorKind::Internal => {}
            }
        }

        assert_eq!(ErrorKind::ALL.len(), 5);
    }

    #[test]
    fn test_that_io_errors_are_retryable() {
        // GIVEN
        let error = DemuxError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset",
        ));

        // WHEN
        let kind = error.kind();

        // THEN
        assert!(kind.is_retryable());
    }

    #[test]
    fn test_that_invalid_input_is_not_retryable() {
        // GIVEN
        let error = DemuxError::InvalidContainer {
            offset: 42,
            message: "bad magic bytes".to_string(),
        };

        // WHEN
        let kind = error.kind();

        // THEN
        assert!(!kind.is_retryable());
    }

    #[test]
    fn test_that_unsupported_codec_error_includes_suggestion() {
        // GIVEN
        let error = DemuxError::UnsupportedCodec {
            codec: "vp8".to_string(),
        };

        // WHEN
        let message = error.to_string();

        // THEN
        assert!(message.contains("vp8"));
        assert!(message.contains("H.264"));
    }

    #[test]
    fn test_that_pipeline_error_delegates_kind_to_inner() {
        // GIVEN
        let inner = DecodeError::ResourceExhausted {
            message: "out of memory".to_string(),
        };
        let error = PipelineError::Decode(inner);

        // WHEN
        let kind = error.kind();

        // THEN
        assert_eq!(kind, ErrorKind::ResourceExhausted);
        assert!(kind.is_retryable());
    }

    #[test]
    fn test_that_other_variant_accepts_custom_errors() {
        // GIVEN
        #[derive(Debug)]
        struct MyCustomError;
        impl fmt::Display for MyCustomError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "custom error")
            }
        }
        impl std::error::Error for MyCustomError {}

        // WHEN
        let error = DemuxError::Other(Box::new(MyCustomError));

        // THEN
        assert_eq!(error.kind(), ErrorKind::Internal);
        assert_eq!(error.to_string(), "custom error");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_that_demux_error_serializes_to_structured_json() {
        // GIVEN
        let error = DemuxError::InvalidContainer {
            offset: 42,
            message: "bad magic bytes".to_string(),
        };

        // WHEN
        let json = serde_json::to_value(&error).unwrap();

        // THEN
        assert_eq!(json["variant"], "InvalidContainer");
        assert_eq!(json["kind"], "invalid_input");
        assert_eq!(json["is_retryable"], false);
        assert_eq!(json["offset"], 42);
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("bad magic bytes"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_that_mux_error_serializes_with_retryable_flag() {
        // GIVEN
        let error = MuxError::ResourceExhausted {
            message: "byte budget exceeded".to_string(),
        };

        // WHEN
        let json = serde_json::to_value(&error).unwrap();

        // THEN
        assert_eq!(json["variant"], "ResourceExhausted");
        assert_eq!(json["kind"], "resource_exhausted");
        assert_eq!(json["is_retryable"], true);
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("byte budget exceeded"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_that_io_error_serializes_with_display_message() {
        // GIVEN
        let error = DemuxError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset",
        ));

        // WHEN
        let json = serde_json::to_value(&error).unwrap();

        // THEN
        assert_eq!(json["variant"], "Io");
        assert_eq!(json["kind"], "io");
        assert_eq!(json["is_retryable"], true);
    }
}
