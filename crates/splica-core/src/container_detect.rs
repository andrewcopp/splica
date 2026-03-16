//! Container format detection from raw bytes.
//!
//! Sniffs the container format from the first bytes of a media file without
//! requiring `Read + Seek`. This is designed for WASM consumers that have a
//! `Uint8Array` header slice rather than a seekable stream.

use crate::media::ContainerFormat;

/// The EBML magic bytes that begin all Matroska-family files.
const EBML_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

/// The EBML element ID for the DocType string.
const DOC_TYPE_ID: [u8; 2] = [0x42, 0x82];

/// Detects the container format from raw header bytes.
///
/// Requires at least 12 bytes to detect MP4, and typically 32-64 bytes to
/// distinguish WebM from MKV. Passing 64 bytes is recommended.
///
/// Returns `None` if the format cannot be determined from the given bytes.
pub fn detect_container(data: &[u8]) -> Option<ContainerFormat> {
    if data.len() < 4 {
        return None;
    }

    // EBML-based: WebM or MKV
    if data[..4] == EBML_MAGIC {
        return Some(detect_ebml_doctype(data));
    }

    // MP4: "ftyp" box at bytes 4-8
    if data.len() >= 8 && &data[4..8] == b"ftyp" {
        return Some(ContainerFormat::Mp4);
    }

    None
}

/// Parses the EBML header to find the DocType element and distinguish WebM
/// from MKV. Falls back to MKV (the superset format) when the DocType cannot
/// be found or is unrecognized.
fn detect_ebml_doctype(data: &[u8]) -> ContainerFormat {
    // Scan for DocType element ID (0x42 0x82) starting after the EBML header ID.
    for i in 4..data.len().saturating_sub(3) {
        if data[i] == DOC_TYPE_ID[0] && data[i + 1] == DOC_TYPE_ID[1] {
            let size_start = i + 2;
            if size_start >= data.len() {
                break;
            }

            let size_byte = data[size_start];
            if size_byte == 0 {
                break;
            }

            // Decode EBML variable-length integer (vint).
            let width = size_byte.leading_zeros() as usize + 1;
            if size_start + width > data.len() {
                break;
            }

            let mut size: u64 = 0;
            for &b in &data[size_start..size_start + width] {
                size = (size << 8) | b as u64;
            }
            // Strip the marker bit.
            let mask = 1u64 << (7 * width);
            size &= mask - 1;

            let str_start = size_start + width;
            let str_end = str_start + size as usize;
            if str_end > data.len() {
                break;
            }

            let doc_type_bytes = &data[str_start..str_end];
            // Trim trailing nulls.
            let end = doc_type_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(doc_type_bytes.len());
            let doc_type = core::str::from_utf8(&doc_type_bytes[..end]).unwrap_or("");

            return match doc_type {
                "webm" => ContainerFormat::WebM,
                _ => ContainerFormat::Mkv,
            };
        }
    }

    // Could not find DocType — default to MKV (the superset).
    ContainerFormat::Mkv
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal EBML header with the given DocType string.
    fn make_ebml_header(doc_type: &str) -> Vec<u8> {
        let dt_bytes = doc_type.as_bytes();
        let dt_len = dt_bytes.len() as u8;
        let doc_type_element_len = 2 + 1 + dt_len;

        let mut buf = Vec::new();
        buf.extend_from_slice(&EBML_MAGIC);
        buf.push(0x80 | doc_type_element_len); // header body size (1-byte vint)
        buf.extend_from_slice(&DOC_TYPE_ID);
        buf.push(0x80 | dt_len); // DocType string size (1-byte vint)
        buf.extend_from_slice(dt_bytes);
        buf
    }

    fn make_mp4_header() -> Vec<u8> {
        let mut buf = vec![0x00, 0x00, 0x00, 0x20]; // box size
        buf.extend_from_slice(b"ftyp");
        buf.extend_from_slice(b"isom"); // brand
        buf
    }

    #[test]
    fn test_that_mp4_is_detected_from_ftyp() {
        let data = make_mp4_header();

        let result = detect_container(&data);

        assert_eq!(result, Some(ContainerFormat::Mp4));
    }

    #[test]
    fn test_that_webm_doctype_is_detected() {
        let data = make_ebml_header("webm");

        let result = detect_container(&data);

        assert_eq!(result, Some(ContainerFormat::WebM));
    }

    #[test]
    fn test_that_matroska_doctype_is_detected_as_mkv() {
        let data = make_ebml_header("matroska");

        let result = detect_container(&data);

        assert_eq!(result, Some(ContainerFormat::Mkv));
    }

    #[test]
    fn test_that_unknown_doctype_defaults_to_mkv() {
        let data = make_ebml_header("unknown");

        let result = detect_container(&data);

        assert_eq!(result, Some(ContainerFormat::Mkv));
    }

    #[test]
    fn test_that_truncated_ebml_defaults_to_mkv() {
        let data = vec![0x1A, 0x45, 0xDF, 0xA3];

        let result = detect_container(&data);

        assert_eq!(result, Some(ContainerFormat::Mkv));
    }

    #[test]
    fn test_that_too_few_bytes_returns_none() {
        let result = detect_container(&[0x00, 0x01, 0x02]);

        assert_eq!(result, None);
    }

    #[test]
    fn test_that_unrecognized_magic_returns_none() {
        let data = vec![0xFF; 16];

        let result = detect_container(&data);

        assert_eq!(result, None);
    }

    #[test]
    fn test_that_empty_input_returns_none() {
        let result = detect_container(&[]);

        assert_eq!(result, None);
    }
}
