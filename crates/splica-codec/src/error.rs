//! Codec-specific error types.

use splica_core::error::{DecodeError, EncodeError, ErrorKind};

/// Errors from codec operations.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("invalid codec configuration: {message}")]
    InvalidConfig { message: String },

    #[error("invalid bitstream: {message}")]
    InvalidBitstream { message: String },

    #[error("decoder error: {message}")]
    DecoderError { message: String },

    #[error("encoder error: {message}")]
    EncoderError { message: String },

    #[error("unsupported codec feature: {message}")]
    Unsupported { message: String },
}

impl CodecError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidConfig { .. } | Self::InvalidBitstream { .. } => ErrorKind::InvalidInput,
            Self::DecoderError { .. } | Self::EncoderError { .. } => ErrorKind::Internal,
            Self::Unsupported { .. } => ErrorKind::UnsupportedFormat,
        }
    }
}

impl From<CodecError> for EncodeError {
    fn from(err: CodecError) -> Self {
        match err {
            CodecError::EncoderError { message } => EncodeError::Other(message.into()),
            CodecError::InvalidConfig { message } => EncodeError::UnsupportedConfig { message },
            other => EncodeError::Other(Box::new(other)),
        }
    }
}

impl From<CodecError> for DecodeError {
    fn from(err: CodecError) -> Self {
        match err {
            CodecError::InvalidBitstream { message } => DecodeError::InvalidBitstream { message },
            CodecError::Unsupported { message } => DecodeError::UnsupportedProfile {
                codec: "H.264".to_string(),
                profile: message,
            },
            other => DecodeError::Other(Box::new(other)),
        }
    }
}
