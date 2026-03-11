//! Error types for the MKV muxer.

/// Errors produced by MKV muxer operations.
#[derive(Debug, thiserror::Error)]
pub enum MkvError {
    #[error("not a valid Matroska file")]
    NotMkv,

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

impl From<MkvError> for splica_core::MuxError {
    fn from(e: MkvError) -> Self {
        splica_core::MuxError::Io(std::io::Error::other(e.to_string()))
    }
}

impl From<MkvError> for splica_core::DemuxError {
    fn from(e: MkvError) -> Self {
        match e {
            MkvError::NotMkv => splica_core::DemuxError::InvalidContainer {
                offset: 0,
                message: "not a valid Matroska file".to_string(),
            },
            MkvError::InvalidElement { offset, message } => {
                splica_core::DemuxError::InvalidContainer { offset, message }
            }
            MkvError::UnexpectedEof { offset } => splica_core::DemuxError::UnexpectedEof { offset },
            MkvError::UnsupportedCodec { codec_id } => {
                splica_core::DemuxError::UnsupportedCodec { codec: codec_id }
            }
            MkvError::MissingElement { name } => splica_core::DemuxError::InvalidContainer {
                offset: 0,
                message: format!("missing required element: {name}"),
            },
            MkvError::Io(e) => splica_core::DemuxError::Io(e),
        }
    }
}
