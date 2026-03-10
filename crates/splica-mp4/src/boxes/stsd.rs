//! Sample Description Box (stsd) parser.
//!
//! Extracts codec identification and configuration from sample entries.
//! Codec-specific sub-boxes (avcC, esds) are stored as opaque bytes
//! for downstream decoders.

use super::{find_box, parse_full_box_header, read_u16, read_u32, FourCC};
use crate::error::Mp4Error;
use bytes::Bytes;

/// Codec configuration extracted from an stsd sample entry.
#[derive(Debug, Clone)]
pub enum CodecConfig {
    Avc1 {
        width: u16,
        height: u16,
        /// Raw avcC box data (contains SPS/PPS).
        avcc: Bytes,
    },
    Hev1 {
        width: u16,
        height: u16,
        /// Raw hvcC box data.
        hvcc: Bytes,
    },
    Av1 {
        width: u16,
        height: u16,
        /// Raw av1C box data.
        av1c: Bytes,
    },
    Mp4a {
        sample_rate: u32,
        channel_count: u16,
        /// Raw esds box data (contains AudioSpecificConfig).
        esds: Bytes,
    },
    Unknown(String),
}

/// Parse an stsd box body, returning the codec config for the first sample entry.
pub fn parse_stsd(data: &[u8], offset: u64) -> Result<CodecConfig, Mp4Error> {
    let (_version, _flags, body) = parse_full_box_header(data, offset)?;

    if body.len() < 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let entry_count = read_u32(body, offset)?;
    if entry_count == 0 {
        return Err(Mp4Error::InvalidBox {
            offset,
            message: "stsd has zero entries".to_string(),
        });
    }

    // Parse the first sample entry
    let entry_data = &body[4..];
    if entry_data.len() < 8 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let entry_size = read_u32(entry_data, offset)? as usize;
    let fourcc = [entry_data[4], entry_data[5], entry_data[6], entry_data[7]];

    if entry_size < 8 || entry_size > entry_data.len() {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let entry_body = &entry_data[8..entry_size];

    match &fourcc {
        b"avc1" | b"avc3" => parse_visual_sample_entry(entry_body, &fourcc, offset),
        b"hev1" | b"hvc1" => parse_visual_sample_entry(entry_body, &fourcc, offset),
        b"av01" => parse_visual_sample_entry(entry_body, &fourcc, offset),
        b"mp4a" => parse_audio_sample_entry(entry_body, offset),
        b"Opus" => Ok(CodecConfig::Unknown("Opus".to_string())),
        _ => {
            let name = String::from_utf8_lossy(&fourcc).to_string();
            Ok(CodecConfig::Unknown(name))
        }
    }
}

fn parse_visual_sample_entry(
    data: &[u8],
    fourcc: &[u8; 4],
    offset: u64,
) -> Result<CodecConfig, Mp4Error> {
    // Visual sample entry layout (after the 8-byte box header):
    // 6 reserved + 2 data_ref_index = 8
    // 2 pre_defined + 2 reserved + 12 pre_defined = 16
    // 2 width + 2 height = 4  (at offset 24)
    // 4 horiz_res + 4 vert_res + 4 reserved = 12
    // 2 frame_count + 32 compressor + 2 depth + 2 pre_defined = 38
    // Total: 8 + 16 + 4 + 12 + 38 = 78 bytes before sub-boxes
    if data.len() < 78 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let width = read_u16(&data[24..], offset)?;
    let height = read_u16(&data[26..], offset)?;

    // Sub-boxes start at offset 78 within the visual sample entry body
    let sub_box_data = &data[78..];

    match fourcc {
        b"avc1" | b"avc3" => {
            let avcc = find_box(sub_box_data, FourCC(*b"avcC"), offset)?
                .map(|b| Bytes::copy_from_slice(b.body))
                .unwrap_or_default();
            Ok(CodecConfig::Avc1 {
                width,
                height,
                avcc,
            })
        }
        b"hev1" | b"hvc1" => {
            let hvcc = find_box(sub_box_data, FourCC(*b"hvcC"), offset)?
                .map(|b| Bytes::copy_from_slice(b.body))
                .unwrap_or_default();
            Ok(CodecConfig::Hev1 {
                width,
                height,
                hvcc,
            })
        }
        b"av01" => {
            let av1c = find_box(sub_box_data, FourCC(*b"av1C"), offset)?
                .map(|b| Bytes::copy_from_slice(b.body))
                .unwrap_or_default();
            Ok(CodecConfig::Av1 {
                width,
                height,
                av1c,
            })
        }
        _ => Ok(CodecConfig::Unknown(
            String::from_utf8_lossy(fourcc).to_string(),
        )),
    }
}

fn parse_audio_sample_entry(data: &[u8], offset: u64) -> Result<CodecConfig, Mp4Error> {
    // Audio sample entry layout (after the 8-byte box header):
    // 6 reserved + 2 data_ref_index = 8
    // 8 reserved (2 x u32) = 8
    // 2 channel_count + 2 sample_size + 2 pre_defined + 2 reserved = 8
    // 4 sample_rate (16.16 fixed-point) = 4
    // Total: 28 bytes before sub-boxes
    if data.len() < 28 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let channel_count = read_u16(&data[16..], offset)?;
    // sample_rate is stored as 16.16 fixed-point at offset 24
    let sample_rate_fp = read_u32(&data[24..], offset)?;
    let sample_rate = sample_rate_fp >> 16;

    // Sub-boxes start at offset 28
    let sub_box_data = &data[28..];
    let esds = find_box(sub_box_data, FourCC(*b"esds"), offset)?
        .map(|b| Bytes::copy_from_slice(b.body))
        .unwrap_or_default();

    Ok(CodecConfig::Mp4a {
        sample_rate,
        channel_count,
        esds,
    })
}
