//! Time-to-Sample Box (stts) parser.

use super::{parse_full_box_header, read_u32};
use crate::error::Mp4Error;

#[derive(Debug, Clone)]
pub struct SttsEntry {
    pub sample_count: u32,
    pub sample_delta: u32,
}

#[derive(Debug)]
pub struct TimeToSampleBox {
    pub entries: Vec<SttsEntry>,
}

pub fn parse_stts(data: &[u8], offset: u64) -> Result<TimeToSampleBox, Mp4Error> {
    let (_version, _flags, body) = parse_full_box_header(data, offset)?;

    if body.len() < 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let entry_count = read_u32(body, offset)? as usize;
    let entries_data = &body[4..];

    if entries_data.len() < entry_count * 8 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let mut entries = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        let pos = i * 8;
        let sample_count = read_u32(&entries_data[pos..], offset)?;
        let sample_delta = read_u32(&entries_data[pos + 4..], offset)?;
        entries.push(SttsEntry {
            sample_count,
            sample_delta,
        });
    }

    Ok(TimeToSampleBox { entries })
}
