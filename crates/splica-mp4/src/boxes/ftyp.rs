//! File Type Box (ftyp) parser.

use crate::error::Mp4Error;

/// Parsed file type box.
#[derive(Debug)]
pub struct FileTypeBox {
    pub major_brand: [u8; 4],
    pub minor_version: u32,
    pub compatible_brands: Vec<[u8; 4]>,
}

/// Parse an ftyp box body.
pub fn parse_ftyp(data: &[u8], offset: u64) -> Result<FileTypeBox, Mp4Error> {
    if data.len() < 8 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let major_brand = [data[0], data[1], data[2], data[3]];
    let minor_version = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

    let brands_data = &data[8..];
    let brand_count = brands_data.len() / 4;
    let mut compatible_brands = Vec::with_capacity(brand_count);
    for i in 0..brand_count {
        let start = i * 4;
        compatible_brands.push([
            brands_data[start],
            brands_data[start + 1],
            brands_data[start + 2],
            brands_data[start + 3],
        ]);
    }

    Ok(FileTypeBox {
        major_brand,
        minor_version,
        compatible_brands,
    })
}
