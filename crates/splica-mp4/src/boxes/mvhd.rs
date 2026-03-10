//! Movie Header Box (mvhd) parser.

use super::{parse_full_box_header, read_u32, read_u64};
use crate::error::Mp4Error;

/// Parsed movie header box.
#[derive(Debug)]
pub struct MovieHeaderBox {
    pub timescale: u32,
    pub duration: u64,
}

/// Parse an mvhd box body.
pub fn parse_mvhd(data: &[u8], offset: u64) -> Result<MovieHeaderBox, Mp4Error> {
    let (version, _flags, body) = parse_full_box_header(data, offset)?;

    match version {
        0 => {
            // version 0: 4 bytes each for creation_time, modification_time, timescale, duration
            if body.len() < 16 {
                return Err(Mp4Error::UnexpectedEof { offset });
            }
            let timescale = read_u32(&body[8..], offset)?;
            let duration = read_u32(&body[12..], offset)? as u64;
            Ok(MovieHeaderBox {
                timescale,
                duration,
            })
        }
        1 => {
            // version 1: 8 bytes each for creation_time, modification_time, then 4 for timescale, 8 for duration
            if body.len() < 28 {
                return Err(Mp4Error::UnexpectedEof { offset });
            }
            let timescale = read_u32(&body[16..], offset)?;
            let duration = read_u64(&body[20..], offset)?;
            Ok(MovieHeaderBox {
                timescale,
                duration,
            })
        }
        _ => Err(Mp4Error::InvalidBox {
            offset,
            message: format!("unsupported mvhd version {version}"),
        }),
    }
}
