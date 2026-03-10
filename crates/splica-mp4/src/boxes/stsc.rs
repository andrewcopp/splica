//! Sample-to-Chunk Box (stsc) parser.

use super::{parse_full_box_header, read_u32};
use crate::error::Mp4Error;

#[derive(Debug, Clone)]
pub struct StscEntry {
    /// First chunk number (1-indexed).
    pub first_chunk: u32,
    pub samples_per_chunk: u32,
    pub sample_description_index: u32,
}

#[derive(Debug)]
pub struct SampleToChunkBox {
    pub entries: Vec<StscEntry>,
}

pub fn parse_stsc(data: &[u8], offset: u64) -> Result<SampleToChunkBox, Mp4Error> {
    let (_version, _flags, body) = parse_full_box_header(data, offset)?;

    if body.len() < 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let entry_count = read_u32(body, offset)? as usize;
    let entries_data = &body[4..];

    if entries_data.len() < entry_count * 12 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let mut entries = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        let pos = i * 12;
        let first_chunk = read_u32(&entries_data[pos..], offset)?;
        let samples_per_chunk = read_u32(&entries_data[pos + 4..], offset)?;
        let sample_description_index = read_u32(&entries_data[pos + 8..], offset)?;
        entries.push(StscEntry {
            first_chunk,
            samples_per_chunk,
            sample_description_index,
        });
    }

    Ok(SampleToChunkBox { entries })
}
