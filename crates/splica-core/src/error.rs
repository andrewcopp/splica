//! Error types and error hierarchy for the splica media processing library.
//!
//! Each pipeline stage has its own error type with domain-relevant variants.
//! All error types expose an [`ErrorKind`] for automated retry/abort decisions.

use std::fmt;

/// Broad categorization of errors for automated retry/abort decisions.
///
/// Platform engineers (like Priya) match on this to decide whether to retry
/// a failed operation or abort immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Returns `true` if the error may be transient and retrying could succeed.
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Io | Self::ResourceExhausted)
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
            Self::Other(_) => ErrorKind::Internal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
