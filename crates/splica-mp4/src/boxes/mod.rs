//! ISO BMFF box header parsing and iteration.
//!
//! All parsing functions are pure: they take byte slices and return typed results.

pub mod ctts;
pub mod ftyp;
pub mod hdlr;
pub mod mdhd;
pub mod mvhd;
pub mod stco;
pub mod stsc;
pub mod stsd;
pub mod stss;
pub mod stsz;
pub mod stts;
pub mod tkhd;

use crate::error::Mp4Error;

/// Four-character code identifying a box type.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCC(pub [u8; 4]);

impl FourCC {
    pub const FTYP: Self = Self(*b"ftyp");
    pub const MOOV: Self = Self(*b"moov");
    pub const MVHD: Self = Self(*b"mvhd");
    pub const TRAK: Self = Self(*b"trak");
    pub const TKHD: Self = Self(*b"tkhd");
    pub const MDIA: Self = Self(*b"mdia");
    pub const MDHD: Self = Self(*b"mdhd");
    pub const HDLR: Self = Self(*b"hdlr");
    pub const MINF: Self = Self(*b"minf");
    pub const STBL: Self = Self(*b"stbl");
    pub const STSD: Self = Self(*b"stsd");
    pub const STTS: Self = Self(*b"stts");
    pub const CTTS: Self = Self(*b"ctts");
    pub const STSC: Self = Self(*b"stsc");
    pub const STSZ: Self = Self(*b"stsz");
    pub const STCO: Self = Self(*b"stco");
    pub const CO64: Self = Self(*b"co64");
    pub const STSS: Self = Self(*b"stss");
    pub const MDAT: Self = Self(*b"mdat");
    pub const UDTA: Self = Self(*b"udta");
    pub const META: Self = Self(*b"meta");
}

impl std::fmt::Debug for FourCC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FourCC(\"{}\")", self)
    }
}

impl std::fmt::Display for FourCC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for &b in &self.0 {
            if b.is_ascii_graphic() || b == b' ' {
                write!(f, "{}", b as char)?;
            } else {
                write!(f, "\\x{b:02x}")?;
            }
        }
        Ok(())
    }
}

/// Parsed box header.
#[derive(Debug, Clone, Copy)]
pub struct BoxHeader {
    /// Four-character code.
    pub box_type: FourCC,
    /// Total box size in bytes (including header).
    pub size: u64,
    /// Size of just the header (8 or 16 bytes).
    pub header_size: u8,
}

impl BoxHeader {
    /// Size of the body (total size minus header).
    pub fn body_size(&self) -> u64 {
        self.size - self.header_size as u64
    }
}

/// Parse a box header from the start of a byte slice.
///
/// Returns the header and the number of bytes consumed.
pub fn parse_box_header(data: &[u8], offset: u64) -> Result<BoxHeader, Mp4Error> {
    if data.len() < 8 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let size_field = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let box_type = FourCC([data[4], data[5], data[6], data[7]]);

    let (size, header_size) = if size_field == 1 {
        // Extended size: next 8 bytes are the real 64-bit size
        if data.len() < 16 {
            return Err(Mp4Error::UnexpectedEof { offset });
        }
        let extended = u64::from_be_bytes([
            data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
        ]);
        (extended, 16u8)
    } else if size_field == 0 {
        // Box extends to end of file — we can't determine from just the header,
        // so we'll use the remaining data length as a stand-in when iterating.
        // For now, return 0 and let the caller handle it.
        (0u64, 8u8)
    } else {
        (size_field as u64, 8u8)
    };

    Ok(BoxHeader {
        box_type,
        size,
        header_size,
    })
}

/// A parsed box: header + body bytes.
#[derive(Debug)]
pub struct ParsedBox<'a> {
    pub header: BoxHeader,
    pub body: &'a [u8],
    /// Offset of this box relative to the start of the parent data.
    pub offset: u64,
}

/// Iterate over sibling boxes within a byte slice.
///
/// Yields each box's header, body bytes, and offset. Skips unknown boxes gracefully.
pub fn iter_boxes(data: &[u8], base_offset: u64) -> BoxIterator<'_> {
    BoxIterator {
        data,
        position: 0,
        base_offset,
    }
}

pub struct BoxIterator<'a> {
    data: &'a [u8],
    position: usize,
    base_offset: u64,
}

impl<'a> Iterator for BoxIterator<'a> {
    type Item = Result<ParsedBox<'a>, Mp4Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.data.len() {
            return None;
        }

        let remaining = &self.data[self.position..];
        if remaining.len() < 8 {
            // Not enough data for another box header — stop iteration
            return None;
        }

        let abs_offset = self.base_offset + self.position as u64;
        let header = match parse_box_header(remaining, abs_offset) {
            Ok(h) => h,
            Err(e) => return Some(Err(e)),
        };

        let total_size = if header.size == 0 {
            // Box extends to end of data
            remaining.len() as u64
        } else {
            header.size
        };

        if total_size < header.header_size as u64 {
            return Some(Err(Mp4Error::InvalidBox {
                offset: abs_offset,
                message: format!(
                    "box '{}' size {} is smaller than header",
                    header.box_type, total_size
                ),
            }));
        }

        if total_size as usize > remaining.len() {
            return Some(Err(Mp4Error::UnexpectedEof { offset: abs_offset }));
        }

        let body_start = header.header_size as usize;
        let body_end = total_size as usize;
        let body = &remaining[body_start..body_end];

        self.position += total_size as usize;

        Some(Ok(ParsedBox {
            header,
            body,
            offset: abs_offset,
        }))
    }
}

/// Find the first box with the given type within a byte slice.
pub fn find_box<'a>(
    data: &'a [u8],
    box_type: FourCC,
    base_offset: u64,
) -> Result<Option<ParsedBox<'a>>, Mp4Error> {
    for result in iter_boxes(data, base_offset) {
        let parsed = result?;
        if parsed.header.box_type == box_type {
            return Ok(Some(parsed));
        }
    }
    Ok(None)
}

/// Find a required box, returning `MissingBox` if not found.
pub fn require_box<'a>(
    data: &'a [u8],
    box_type: FourCC,
    base_offset: u64,
    name: &'static str,
) -> Result<ParsedBox<'a>, Mp4Error> {
    find_box(data, box_type, base_offset)?.ok_or(Mp4Error::MissingBox { name })
}

/// Parse a full-box header (version + flags) from the start of a box body.
/// Returns (version, flags, remaining body after the 4-byte full-box header).
pub fn parse_full_box_header(body: &[u8], offset: u64) -> Result<(u8, u32, &[u8]), Mp4Error> {
    if body.len() < 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }
    let version = body[0];
    let flags = u32::from_be_bytes([0, body[1], body[2], body[3]]);
    Ok((version, flags, &body[4..]))
}

/// Read a big-endian u16 from a byte slice.
pub fn read_u16(data: &[u8], offset: u64) -> Result<u16, Mp4Error> {
    if data.len() < 2 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }
    Ok(u16::from_be_bytes([data[0], data[1]]))
}

/// Read a big-endian u32 from a byte slice.
pub fn read_u32(data: &[u8], offset: u64) -> Result<u32, Mp4Error> {
    if data.len() < 4 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }
    Ok(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
}

/// Read a big-endian u64 from a byte slice.
pub fn read_u64(data: &[u8], offset: u64) -> Result<u64, Mp4Error> {
    if data.len() < 8 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }
    Ok(u64::from_be_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_box_header_parses_standard_size() {
        // GIVEN — ftyp box, 20 bytes total
        let mut data = vec![0u8; 20];
        data[0..4].copy_from_slice(&20u32.to_be_bytes());
        data[4..8].copy_from_slice(b"ftyp");

        // WHEN
        let header = parse_box_header(&data, 0).unwrap();

        // THEN
        assert_eq!(header.box_type, FourCC::FTYP);
        assert_eq!(header.size, 20);
        assert_eq!(header.header_size, 8);
        assert_eq!(header.body_size(), 12);
    }

    #[test]
    fn test_that_box_header_parses_extended_size() {
        // GIVEN — box with extended size
        let mut data = vec![0u8; 24];
        data[0..4].copy_from_slice(&1u32.to_be_bytes()); // size=1 means extended
        data[4..8].copy_from_slice(b"mdat");
        data[8..16].copy_from_slice(&24u64.to_be_bytes());

        // WHEN
        let header = parse_box_header(&data, 0).unwrap();

        // THEN
        assert_eq!(header.box_type, FourCC::MDAT);
        assert_eq!(header.size, 24);
        assert_eq!(header.header_size, 16);
    }

    #[test]
    fn test_that_iter_boxes_walks_siblings() {
        // GIVEN — two sibling boxes
        let mut data = vec![0u8; 28];
        // Box 1: 12 bytes total, type "abcd"
        data[0..4].copy_from_slice(&12u32.to_be_bytes());
        data[4..8].copy_from_slice(b"abcd");
        // Box 2: 16 bytes total, type "efgh"
        data[12..16].copy_from_slice(&16u32.to_be_bytes());
        data[16..20].copy_from_slice(b"efgh");

        // WHEN
        let boxes: Vec<_> = iter_boxes(&data, 0).collect::<Result<_, _>>().unwrap();

        // THEN
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0].header.box_type, FourCC(*b"abcd"));
        assert_eq!(boxes[0].body.len(), 4);
        assert_eq!(boxes[1].header.box_type, FourCC(*b"efgh"));
        assert_eq!(boxes[1].body.len(), 8);
    }

    #[test]
    fn test_that_truncated_header_returns_eof() {
        // GIVEN — only 4 bytes
        let data = [0u8; 4];

        // WHEN
        let result = parse_box_header(&data, 42);

        // THEN
        assert!(matches!(
            result,
            Err(Mp4Error::UnexpectedEof { offset: 42 })
        ));
    }

    #[test]
    fn test_that_fourcc_displays_ascii() {
        assert_eq!(FourCC::FTYP.to_string(), "ftyp");
        assert_eq!(FourCC::MOOV.to_string(), "moov");
    }
}
