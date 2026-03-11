//! EBML encoding helpers for the MKV muxer.
//!
//! These are the write-side primitives needed for building EBML elements.
//! Reading primitives are not included — add them when an MKV demuxer is needed.

/// Encodes an EBML element ID into bytes (preserving marker bits).
pub fn encode_element_id(id: u32) -> Vec<u8> {
    if id <= 0xFF {
        vec![id as u8]
    } else if id <= 0xFFFF {
        vec![(id >> 8) as u8, id as u8]
    } else if id <= 0xFF_FFFF {
        vec![(id >> 16) as u8, (id >> 8) as u8, id as u8]
    } else {
        vec![
            (id >> 24) as u8,
            (id >> 16) as u8,
            (id >> 8) as u8,
            id as u8,
        ]
    }
}

/// Encodes a data size as an EBML vint (with marker bit).
pub fn encode_data_size(size: u64) -> Vec<u8> {
    if size < 0x7F {
        vec![(size as u8) | 0x80]
    } else if size < 0x3FFF {
        let val = size | 0x4000;
        vec![(val >> 8) as u8, val as u8]
    } else if size < 0x1F_FFFF {
        let val = size | 0x20_0000;
        vec![(val >> 16) as u8, (val >> 8) as u8, val as u8]
    } else {
        let val = size | 0x10_000000;
        vec![
            (val >> 24) as u8,
            (val >> 16) as u8,
            (val >> 8) as u8,
            val as u8,
        ]
    }
}

/// Builds a complete EBML element: ID + data size + body.
pub fn build_element(id: u32, body: &[u8]) -> Vec<u8> {
    let mut out = encode_element_id(id);
    out.extend_from_slice(&encode_data_size(body.len() as u64));
    out.extend_from_slice(body);
    out
}

/// Encodes a `u64` as a big-endian unsigned integer with minimal byte width.
fn encode_uint(value: u64) -> Vec<u8> {
    if value == 0 {
        vec![0]
    } else if value <= 0xFF {
        vec![value as u8]
    } else if value <= 0xFFFF {
        vec![(value >> 8) as u8, value as u8]
    } else if value <= 0xFF_FFFF {
        vec![(value >> 16) as u8, (value >> 8) as u8, value as u8]
    } else if value <= 0xFFFF_FFFF {
        vec![
            (value >> 24) as u8,
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        ]
    } else {
        value.to_be_bytes().to_vec()
    }
}

/// Builds an EBML unsigned integer element.
pub fn uint_element(id: u32, value: u64) -> Vec<u8> {
    build_element(id, &encode_uint(value))
}

/// Builds an EBML string element.
pub fn string_element(id: u32, s: &str) -> Vec<u8> {
    build_element(id, s.as_bytes())
}

/// Builds an EBML 8-byte float element.
pub fn float_element(id: u32, value: f64) -> Vec<u8> {
    build_element(id, &value.to_be_bytes())
}
