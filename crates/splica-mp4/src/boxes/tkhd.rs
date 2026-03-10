//! Track Header Box (tkhd) parser.

use super::{parse_full_box_header, read_u32, read_u64};
use crate::error::Mp4Error;

/// Parsed track header box.
#[derive(Debug)]
pub struct TrackHeaderBox {
    pub track_id: u32,
    pub duration: u64,
    /// Width in pixels (converted from 16.16 fixed-point).
    pub width: u32,
    /// Height in pixels (converted from 16.16 fixed-point).
    pub height: u32,
}

/// Parse a tkhd box body.
pub fn parse_tkhd(data: &[u8], offset: u64) -> Result<TrackHeaderBox, Mp4Error> {
    let (version, _flags, body) = parse_full_box_header(data, offset)?;

    match version {
        0 => {
            // v0: creation(4) + modification(4) + track_id(4) + reserved(4) + duration(4) = 20
            // then reserved(8) + layer(2) + alt_group(2) + volume(2) + reserved(2) + matrix(36) + width(4) + height(4) = 60
            if body.len() < 80 {
                return Err(Mp4Error::UnexpectedEof { offset });
            }
            let track_id = read_u32(&body[8..], offset)?;
            let duration = read_u32(&body[16..], offset)? as u64;
            let width_fp = read_u32(&body[72..], offset)?;
            let height_fp = read_u32(&body[76..], offset)?;
            Ok(TrackHeaderBox {
                track_id,
                duration,
                width: width_fp >> 16,
                height: height_fp >> 16,
            })
        }
        1 => {
            // v1: creation(8) + modification(8) + track_id(4) + reserved(4) + duration(8) = 32
            // then same 60 bytes of remaining fields
            if body.len() < 92 {
                return Err(Mp4Error::UnexpectedEof { offset });
            }
            let track_id = read_u32(&body[16..], offset)?;
            let duration = read_u64(&body[24..], offset)?;
            let width_fp = read_u32(&body[84..], offset)?;
            let height_fp = read_u32(&body[88..], offset)?;
            Ok(TrackHeaderBox {
                track_id,
                duration,
                width: width_fp >> 16,
                height: height_fp >> 16,
            })
        }
        _ => Err(Mp4Error::InvalidBox {
            offset,
            message: format!("unsupported tkhd version {version}"),
        }),
    }
}
