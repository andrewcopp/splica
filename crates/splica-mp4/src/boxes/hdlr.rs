//! Handler Reference Box (hdlr) parser.

use super::parse_full_box_header;
use crate::error::Mp4Error;

/// Parsed handler box.
#[derive(Debug)]
pub struct HandlerBox {
    /// Handler type fourcc: b"vide", b"soun", b"hint", etc.
    pub handler_type: [u8; 4],
}

/// Parse an hdlr box body.
pub fn parse_hdlr(data: &[u8], offset: u64) -> Result<HandlerBox, Mp4Error> {
    let (_version, _flags, body) = parse_full_box_header(data, offset)?;

    // pre_defined(4) + handler_type(4)
    if body.len() < 8 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let handler_type = [body[4], body[5], body[6], body[7]];
    Ok(HandlerBox { handler_type })
}
