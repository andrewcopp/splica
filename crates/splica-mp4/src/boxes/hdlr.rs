//! Handler Reference Box (hdlr) parser.

use super::parse_full_box_header;
use crate::error::Mp4Error;

/// Handler type from an hdlr box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerType {
    /// Video track (`b"vide"`).
    Video,
    /// Audio track (`b"soun"`).
    Audio,
    /// Subtitle track (`b"sbtl"` or `b"text"`).
    Subtitle,
    /// Any other handler type.
    Other([u8; 4]),
}

impl HandlerType {
    /// Returns the four-byte representation for box building.
    pub fn as_bytes(&self) -> &[u8; 4] {
        match self {
            HandlerType::Video => b"vide",
            HandlerType::Audio => b"soun",
            HandlerType::Subtitle => b"sbtl",
            HandlerType::Other(fourcc) => fourcc,
        }
    }

    fn from_bytes(bytes: [u8; 4]) -> Self {
        match &bytes {
            b"vide" => HandlerType::Video,
            b"soun" => HandlerType::Audio,
            b"sbtl" | b"text" | b"subt" => HandlerType::Subtitle,
            _ => HandlerType::Other(bytes),
        }
    }
}

/// Parsed handler box.
#[derive(Debug)]
pub struct HandlerBox {
    /// Handler type: video, audio, or other.
    pub handler_type: HandlerType,
}

/// Parse an hdlr box body.
pub fn parse_hdlr(data: &[u8], offset: u64) -> Result<HandlerBox, Mp4Error> {
    let (_version, _flags, body) = parse_full_box_header(data, offset)?;

    // pre_defined(4) + handler_type(4)
    if body.len() < 8 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let fourcc = [body[4], body[5], body[6], body[7]];
    Ok(HandlerBox {
        handler_type: HandlerType::from_bytes(fourcc),
    })
}
