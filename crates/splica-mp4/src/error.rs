//! MP4-specific error types.

use splica_core::DemuxError;

/// Errors produced by MP4 parsing and demuxing operations.
#[derive(Debug, thiserror::Error)]
pub enum Mp4Error {
    #[error("not an MP4 file: missing or invalid ftyp box")]
    NotMp4,

    #[error("invalid box at offset {offset}: {message}")]
    InvalidBox { offset: u64, message: String },

    #[error("unexpected end of file at offset {offset}")]
    UnexpectedEof { offset: u64 },

    #[error("unsupported codec: {fourcc}")]
    UnsupportedCodec { fourcc: String },

    #[error("missing required box: {name}")]
    MissingBox { name: &'static str },

    #[error("resource exhausted: {message}")]
    ResourceExhausted { message: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<Mp4Error> for DemuxError {
    fn from(e: Mp4Error) -> Self {
        match e {
            Mp4Error::NotMp4 => DemuxError::InvalidContainer {
                offset: 0,
                message: "not an MP4 file".to_string(),
            },
            Mp4Error::InvalidBox { offset, message } => {
                DemuxError::InvalidContainer { offset, message }
            }
            Mp4Error::UnexpectedEof { offset } => DemuxError::UnexpectedEof { offset },
            Mp4Error::UnsupportedCodec { fourcc } => DemuxError::UnsupportedCodec { codec: fourcc },
            Mp4Error::MissingBox { name } => DemuxError::InvalidContainer {
                offset: 0,
                message: format!("missing required box: {name}"),
            },
            Mp4Error::ResourceExhausted { message } => DemuxError::Other(message.into()),
            Mp4Error::Io(e) => DemuxError::Io(e),
        }
    }
}
