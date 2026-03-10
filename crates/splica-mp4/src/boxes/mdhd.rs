//! Media Header Box (mdhd) parser.

use super::{parse_full_box_header, read_u32, read_u64};
use crate::error::Mp4Error;

/// Parsed media header box.
#[derive(Debug)]
pub struct MediaHeaderBox {
    pub timescale: u32,
    pub duration: u64,
}

/// Parse an mdhd box body.
pub fn parse_mdhd(data: &[u8], offset: u64) -> Result<MediaHeaderBox, Mp4Error> {
    let (version, _flags, body) = parse_full_box_header(data, offset)?;

    match version {
        0 => {
            if body.len() < 16 {
                return Err(Mp4Error::UnexpectedEof { offset });
            }
            let timescale = read_u32(&body[8..], offset)?;
            let duration = read_u32(&body[12..], offset)? as u64;
            Ok(MediaHeaderBox {
                timescale,
                duration,
            })
        }
        1 => {
            if body.len() < 28 {
                return Err(Mp4Error::UnexpectedEof { offset });
            }
            let timescale = read_u32(&body[16..], offset)?;
            let duration = read_u64(&body[20..], offset)?;
            Ok(MediaHeaderBox {
                timescale,
                duration,
            })
        }
        _ => Err(Mp4Error::InvalidBox {
            offset,
            message: format!("unsupported mdhd version {version}"),
        }),
    }
}
