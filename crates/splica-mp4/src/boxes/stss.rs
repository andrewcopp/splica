//! Sync Sample Box (stss) parser.

use super::{parse_full_box_header, read_u32};
use crate::error::Mp4Error;

/// Parsed sync sample box. Contains 1-indexed sample numbers of keyframes.
#[derive(Debug)]
pub struct SyncSampleBox {
    /// 1-indexed sample numbers that are sync (key) frames.
    pub sync_samples: Vec<u32>,
}

/// Parse an stss box body.
pub fn parse_stss(data: &[u8], offset: u64) -> Result<SyncSampleBox, Mp4Error> {
    let (_version, _flags, body) = parse_full_box_header(data, offset)?;

    if body.len() < 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let entry_count = read_u32(body, offset)? as usize;
    let entries_data = &body[4..];

    if entries_data.len() < entry_count * 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let mut sync_samples = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        sync_samples.push(read_u32(&entries_data[i * 4..], offset)?);
    }

    Ok(SyncSampleBox { sync_samples })
}
