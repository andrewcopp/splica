use std::io::{Read, Seek, SeekFrom};

use miette::{Context, IntoDiagnostic, Result};

// ---------------------------------------------------------------------------
// Format detection
// ---------------------------------------------------------------------------

/// Detected input container format (from magic bytes and EBML DocType).
pub(crate) enum DetectedFormat {
    Mp4,
    WebM,
    Mkv,
}

/// Sniffs the container format from the first few bytes of a file.
///
/// For EBML-based formats (WebM and MKV), reads the DocType element to
/// distinguish them: "webm" maps to WebM, "matroska" maps to MKV, and
/// unrecognized EBML DocTypes default to MKV (the superset format).
pub(crate) fn detect_format(file: &mut (impl Read + Seek)) -> Result<DetectedFormat> {
    let mut magic = [0u8; 12];
    let bytes_read = file
        .read(&mut magic)
        .into_diagnostic()
        .wrap_err("could not read file header")?;
    file.seek(SeekFrom::Start(0))
        .into_diagnostic()
        .wrap_err("could not seek back to start")?;

    if bytes_read < 4 {
        return Err(miette::miette!(
            "file too small to detect format ({bytes_read} bytes)"
        ));
    }

    // WebM/MKV (Matroska): EBML header starts with 0x1A 0x45 0xDF 0xA3
    if magic[0] == 0x1A && magic[1] == 0x45 && magic[2] == 0xDF && magic[3] == 0xA3 {
        let format = detect_ebml_doctype(file).unwrap_or(DetectedFormat::Mkv);
        return Ok(format);
    }

    // MP4: "ftyp" box at bytes 4-7
    if bytes_read >= 8 && &magic[4..8] == b"ftyp" {
        return Ok(DetectedFormat::Mp4);
    }

    Err(miette::miette!(
        "unsupported container format — splica supports MP4, WebM, and MKV"
    ))
}

/// Reads the EBML header to extract the DocType and determine whether the
/// file is WebM or MKV. Resets the reader to position 0 before returning.
fn detect_ebml_doctype(file: &mut (impl Read + Seek)) -> Result<DetectedFormat> {
    file.seek(SeekFrom::Start(0))
        .into_diagnostic()
        .wrap_err("could not seek to start for DocType detection")?;

    // Read enough of the EBML header to find the DocType element.
    // The EBML header is typically small (under 64 bytes).
    let mut buf = [0u8; 128];
    let n = file
        .read(&mut buf)
        .into_diagnostic()
        .wrap_err("could not read EBML header")?;
    file.seek(SeekFrom::Start(0))
        .into_diagnostic()
        .wrap_err("could not seek back to start after DocType detection")?;

    let data = &buf[..n];

    // Search for DocType element ID (0x4282) within the EBML header body.
    // The EBML header element starts at byte 0 with ID 0x1A45DFA3.
    // We scan for the 2-byte DocType element ID and read its string value.
    let doc_type_id: [u8; 2] = [0x42, 0x82];
    for i in 4..data.len().saturating_sub(3) {
        if data[i] == doc_type_id[0] && data[i + 1] == doc_type_id[1] {
            // Found DocType element ID. Next byte(s) are the EBML vint size.
            let size_start = i + 2;
            if size_start >= data.len() {
                break;
            }
            let size_byte = data[size_start];
            if size_byte == 0 {
                break;
            }
            // For a 1-byte vint, the size is size_byte & 0x7F
            let width = size_byte.leading_zeros() as usize + 1;
            if size_start + width > data.len() {
                break;
            }
            let mut size: u64 = 0;
            for &b in &data[size_start..size_start + width] {
                size = (size << 8) | b as u64;
            }
            // Strip marker bit
            let mask = 1u64 << (7 * width);
            size &= mask - 1;

            let str_start = size_start + width;
            let str_end = str_start + size as usize;
            if str_end > data.len() {
                break;
            }
            let doc_type_bytes = &data[str_start..str_end];
            // Trim trailing nulls
            let end = doc_type_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(doc_type_bytes.len());
            let doc_type = std::str::from_utf8(&doc_type_bytes[..end]).unwrap_or("");

            return match doc_type {
                "webm" => Ok(DetectedFormat::WebM),
                "matroska" => Ok(DetectedFormat::Mkv),
                _ => Ok(DetectedFormat::Mkv), // MKV is the superset
            };
        }
    }

    // Could not find DocType — default to MKV (the superset)
    Ok(DetectedFormat::Mkv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Builds a minimal EBML header with the given DocType string.
    ///
    /// Layout: EBML header ID (4 bytes) + EBML header size vint +
    ///         DocType element ID (2 bytes) + DocType size vint + DocType string
    fn make_ebml_header(doc_type: &str) -> Vec<u8> {
        let dt_bytes = doc_type.as_bytes();
        let dt_len = dt_bytes.len() as u8;

        // DocType element: ID (0x42, 0x82) + 1-byte vint size + string
        let doc_type_element_len = 2 + 1 + dt_len;

        // EBML header: ID (4 bytes) + 1-byte vint total size + DocType element
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]); // EBML header ID
        buf.push(0x80 | doc_type_element_len); // header body size (1-byte vint)
        buf.extend_from_slice(&[0x42, 0x82]); // DocType element ID
        buf.push(0x80 | dt_len); // DocType string size (1-byte vint)
        buf.extend_from_slice(dt_bytes);
        buf
    }

    #[test]
    fn test_that_webm_doctype_is_detected() {
        let data = make_ebml_header("webm");

        let mut cursor = Cursor::new(data);
        let result = detect_format(&mut cursor).unwrap();

        assert!(matches!(result, DetectedFormat::WebM));
    }

    #[test]
    fn test_that_matroska_doctype_is_detected_as_mkv() {
        let data = make_ebml_header("matroska");

        let mut cursor = Cursor::new(data);
        let result = detect_format(&mut cursor).unwrap();

        assert!(matches!(result, DetectedFormat::Mkv));
    }

    #[test]
    fn test_that_unknown_doctype_defaults_to_mkv() {
        let data = make_ebml_header("unknown");

        let mut cursor = Cursor::new(data);
        let result = detect_format(&mut cursor).unwrap();

        assert!(matches!(result, DetectedFormat::Mkv));
    }

    #[test]
    fn test_that_truncated_ebml_header_defaults_to_mkv() {
        // EBML magic bytes only, no DocType element following
        let data = vec![0x1A, 0x45, 0xDF, 0xA3];

        let mut cursor = Cursor::new(data);
        let result = detect_format(&mut cursor).unwrap();

        assert!(matches!(result, DetectedFormat::Mkv));
    }

    #[test]
    fn test_that_empty_input_returns_error() {
        let data: Vec<u8> = vec![];

        let mut cursor = Cursor::new(data);
        let result = detect_format(&mut cursor);

        assert!(result.is_err());
    }
}
