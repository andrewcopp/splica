//! Chunk Offset Box (stco / co64) parser.

use super::{parse_full_box_header, read_u32, read_u64};
use crate::error::Mp4Error;

/// Parsed chunk offsets (handles both stco and co64).
#[derive(Debug)]
pub struct ChunkOffsetBox {
    pub offsets: Vec<u64>,
}

/// Parse an stco box body (32-bit chunk offsets).
pub fn parse_stco(data: &[u8], offset: u64) -> Result<ChunkOffsetBox, Mp4Error> {
    let (_version, _flags, body) = parse_full_box_header(data, offset)?;

    if body.len() < 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let entry_count = read_u32(body, offset)? as usize;
    let entries_data = &body[4..];

    if entries_data.len() < entry_count * 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let mut offsets = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        offsets.push(read_u32(&entries_data[i * 4..], offset)? as u64);
    }

    Ok(ChunkOffsetBox { offsets })
}

/// Parse a co64 box body (64-bit chunk offsets).
pub fn parse_co64(data: &[u8], offset: u64) -> Result<ChunkOffsetBox, Mp4Error> {
    let (_version, _flags, body) = parse_full_box_header(data, offset)?;

    if body.len() < 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let entry_count = read_u32(body, offset)? as usize;
    let entries_data = &body[4..];

    if entries_data.len() < entry_count * 8 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let mut offsets = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        offsets.push(read_u64(&entries_data[i * 8..], offset)?);
    }

    Ok(ChunkOffsetBox { offsets })
}
