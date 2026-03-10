//! Error types for the WebM demuxer.

/// Errors produced by WebM demuxer operations.
#[derive(Debug, thiserror::Error)]
pub enum WebmError {
    #[error("not a valid EBML/WebM file")]
    NotWebm,

    #[error("invalid EBML element at offset {offset}: {message}")]
    InvalidElement { offset: u64, message: String },

    #[error("unexpected end of file at offset {offset}")]
    UnexpectedEof { offset: u64 },

    #[error("unsupported codec: {codec_id}")]
    UnsupportedCodec { codec_id: String },

    #[error("missing required element: {name}")]
    MissingElement { name: &'static str },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<WebmError> for splica_core::MuxError {
    fn from(e: WebmError) -> Self {
        splica_core::MuxError::Io(std::io::Error::other(e.to_string()))
    }
}

impl From<WebmError> for splica_core::DemuxError {
    fn from(e: WebmError) -> Self {
        match e {
            WebmError::NotWebm => splica_core::DemuxError::InvalidContainer {
                offset: 0,
                message: "not a valid EBML/WebM file".to_string(),
            },
            WebmError::InvalidElement { offset, message } => {
                splica_core::DemuxError::InvalidContainer { offset, message }
            }
            WebmError::UnexpectedEof { offset } => {
                splica_core::DemuxError::UnexpectedEof { offset }
            }
            WebmError::UnsupportedCodec { codec_id } => {
                splica_core::DemuxError::UnsupportedCodec { codec: codec_id }
            }
            WebmError::MissingElement { name } => splica_core::DemuxError::InvalidContainer {
                offset: 0,
                message: format!("missing required element: {name}"),
            },
            WebmError::Io(e) => splica_core::DemuxError::Io(e),
        }
    }
}
