//! Composition Time Offset Box (ctts) parser.

use super::{parse_full_box_header, read_u32};
use crate::error::Mp4Error;

#[derive(Debug, Clone)]
pub struct CttsEntry {
    pub sample_count: u32,
    /// Composition offset. Version 0 stores as u32, version 1 as i32.
    /// We store as i32 to handle both.
    pub sample_offset: i32,
}

#[derive(Debug)]
pub struct CompositionOffsetBox {
    pub entries: Vec<CttsEntry>,
}

pub fn parse_ctts(data: &[u8], offset: u64) -> Result<CompositionOffsetBox, Mp4Error> {
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
        let raw_offset = read_u32(&entries_data[pos + 4..], offset)?;
        // Both v0 (unsigned) and v1 (signed) are stored as u32 on the wire;
        // reinterpreting as i32 gives the correct value for both versions.
        let sample_offset = raw_offset as i32;
        entries.push(CttsEntry {
            sample_count,
            sample_offset,
        });
    }

    Ok(CompositionOffsetBox { entries })
}
