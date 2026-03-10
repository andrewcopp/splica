//! EBML (Extensible Binary Meta Language) low-level parsing primitives.
//!
//! EBML uses variable-length integers (vints) for both element IDs and sizes.
//! A vint encodes its length in the leading bits of the first byte:
//! - `1xxx xxxx` → 1 byte
//! - `01xx xxxx xxxx xxxx` → 2 bytes
//! - `001x xxxx ...` → 3 bytes
//! - etc., up to 8 bytes

use crate::error::WebmError;

/// Maximum vint width in bytes.
const MAX_VINT_WIDTH: usize = 8;

/// Reads a variable-length integer from `data` at the given offset.
///
/// Returns `(value, bytes_consumed)`. The value has the length marker bits
/// stripped (for sizes) or preserved (for element IDs, use `read_element_id`).
pub fn read_vint(data: &[u8], offset: u64) -> Result<(u64, usize), WebmError> {
    if data.is_empty() {
        return Err(WebmError::UnexpectedEof { offset });
    }

    let first = data[0];
    if first == 0 {
        return Err(WebmError::InvalidElement {
            offset,
            message: "vint first byte is zero".to_string(),
        });
    }

    // Count leading zeros to determine width
    let width = first.leading_zeros() as usize + 1;

    if width > MAX_VINT_WIDTH {
        return Err(WebmError::InvalidElement {
            offset,
            message: format!("vint width {width} exceeds maximum {MAX_VINT_WIDTH}"),
        });
    }

    if data.len() < width {
        return Err(WebmError::UnexpectedEof { offset });
    }

    // Build the raw value from all bytes
    let mut value: u64 = 0;
    for &byte in &data[..width] {
        value = (value << 8) | byte as u64;
    }

    Ok((value, width))
}

/// Reads an EBML element ID (vint with marker bits preserved).
///
/// Element IDs keep the leading 1-bit that marks the vint width, so
/// `0x1A` is the ID `0x1A`, not `0x1A` with the marker stripped.
pub fn read_element_id(data: &[u8], offset: u64) -> Result<(u32, usize), WebmError> {
    let (value, width) = read_vint(data, offset)?;

    // Element IDs are at most 4 bytes (Class A through D)
    if width > 4 {
        return Err(WebmError::InvalidElement {
            offset,
            message: format!("element ID width {width} exceeds 4 bytes"),
        });
    }

    Ok((value as u32, width))
}

/// Reads an EBML data size (vint with marker bit stripped).
///
/// Returns the payload size. A value of all-1s after stripping the marker
/// indicates "unknown size" (returned as `None`).
pub fn read_data_size(data: &[u8], offset: u64) -> Result<(Option<u64>, usize), WebmError> {
    let (raw, width) = read_vint(data, offset)?;

    // Strip the leading marker bit
    let mask = 1u64 << (7 * width);
    let value = raw & (mask - 1);

    // Check for "unknown size" (all data bits set to 1)
    let unknown_marker = mask - 1;
    if value == unknown_marker {
        return Ok((None, width));
    }

    Ok((Some(value), width))
}

/// Reads an unsigned integer from EBML element data (big-endian, variable width).
pub fn read_uint(data: &[u8], offset: u64) -> Result<u64, WebmError> {
    if data.len() > 8 {
        return Err(WebmError::InvalidElement {
            offset,
            message: "unsigned integer exceeds 8 bytes".to_string(),
        });
    }
    let mut value: u64 = 0;
    for &byte in data {
        value = (value << 8) | byte as u64;
    }
    Ok(value)
}

/// Reads a float from EBML element data (big-endian, 4 or 8 bytes).
pub fn read_float(data: &[u8], offset: u64) -> Result<f64, WebmError> {
    match data.len() {
        0 => Ok(0.0),
        4 => {
            let bits = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            Ok(f32::from_bits(bits) as f64)
        }
        8 => {
            let bits = u64::from_be_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]);
            Ok(f64::from_bits(bits))
        }
        _ => Err(WebmError::InvalidElement {
            offset,
            message: format!("float must be 0, 4, or 8 bytes, got {}", data.len()),
        }),
    }
}

/// Reads a UTF-8 string from EBML element data.
pub fn read_string(data: &[u8], offset: u64) -> Result<String, WebmError> {
    // EBML strings may have trailing null bytes
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8(data[..end].to_vec()).map_err(|_| WebmError::InvalidElement {
        offset,
        message: "invalid UTF-8 in string element".to_string(),
    })
}

/// A parsed EBML element header (ID + data size).
#[derive(Debug, Clone, Copy)]
pub struct ElementHeader {
    /// Element ID (with vint marker bits preserved).
    pub id: u32,
    /// Payload size in bytes (`None` for unknown-size elements).
    pub data_size: Option<u64>,
    /// Total header size (ID bytes + size bytes).
    pub header_size: usize,
}

/// Parses an EBML element header from `data`.
pub fn parse_element_header(data: &[u8], offset: u64) -> Result<ElementHeader, WebmError> {
    let (id, id_len) = read_element_id(data, offset)?;

    if data.len() < id_len {
        return Err(WebmError::UnexpectedEof { offset });
    }

    let (data_size, size_len) = read_data_size(&data[id_len..], offset + id_len as u64)?;

    Ok(ElementHeader {
        id,
        data_size,
        header_size: id_len + size_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_vint_1byte_parses() {
        // 0x81 = 1000_0001 → width 1, raw value 0x81
        let (val, len) = read_vint(&[0x81], 0).unwrap();
        assert_eq!(val, 0x81);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_that_vint_2byte_parses() {
        // 0x40, 0x02 = 0100_0000 0000_0010 → width 2
        let (val, len) = read_vint(&[0x40, 0x02], 0).unwrap();
        assert_eq!(len, 2);
        assert_eq!(val, 0x4002);
    }

    #[test]
    fn test_that_vint_zero_is_rejected() {
        let result = read_vint(&[0x00], 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_that_data_size_strips_marker() {
        // 0x82 = 1000_0010 → width 1, value after stripping = 0x02
        let (size, len) = read_data_size(&[0x82], 0).unwrap();
        assert_eq!(size, Some(2));
        assert_eq!(len, 1);
    }

    #[test]
    fn test_that_unknown_size_returns_none() {
        // 0xFF = all bits set → unknown size for 1-byte vint
        let (size, len) = read_data_size(&[0xFF], 0).unwrap();
        assert_eq!(size, None);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_that_element_id_parses_1a45dfa3() {
        // EBML header ID: 0x1A45DFA3 (4-byte class D element)
        let data = [0x1A, 0x45, 0xDF, 0xA3];
        let (id, len) = read_element_id(&data, 0).unwrap();
        assert_eq!(id, 0x1A45DFA3);
        assert_eq!(len, 4);
    }

    #[test]
    fn test_that_uint_reads_correctly() {
        assert_eq!(read_uint(&[0x01], 0).unwrap(), 1);
        assert_eq!(read_uint(&[0x01, 0x00], 0).unwrap(), 256);
        assert_eq!(read_uint(&[], 0).unwrap(), 0);
    }

    #[test]
    fn test_that_float_reads_4byte() {
        let val: f32 = 1.0;
        let bytes = val.to_be_bytes();
        let result = read_float(&bytes, 0).unwrap();
        assert!((result - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_that_float_reads_8byte() {
        let val: f64 = 48000.0;
        let bytes = val.to_be_bytes();
        let result = read_float(&bytes, 0).unwrap();
        assert!((result - 48000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_that_string_trims_null() {
        let data = b"hello\0\0";
        let s = read_string(data, 0).unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_that_element_header_parses() {
        // Element ID 0xA3 (SimpleBlock, 1-byte), size 0x05
        let data = [0xA3, 0x85]; // 0x85 = size vint for 5
        let header = parse_element_header(&data, 0).unwrap();
        assert_eq!(header.id, 0xA3);
        assert_eq!(header.data_size, Some(5));
        assert_eq!(header.header_size, 2);
    }
}
