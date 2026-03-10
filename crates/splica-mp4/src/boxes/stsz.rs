//! Sample Size Box (stsz) parser.

use super::{parse_full_box_header, read_u32};
use crate::error::Mp4Error;

#[derive(Debug)]
pub struct SampleSizeBox {
    /// If non-zero, all samples have this size and `sample_sizes` is empty.
    pub default_sample_size: u32,
    /// Per-sample sizes (empty if `default_sample_size > 0`).
    pub sample_sizes: Vec<u32>,
    /// Total number of samples.
    pub sample_count: u32,
}

pub fn parse_stsz(data: &[u8], offset: u64) -> Result<SampleSizeBox, Mp4Error> {
    let (_version, _flags, body) = parse_full_box_header(data, offset)?;

    if body.len() < 8 {
        return Err(Mp4Error::UnexpectedEof { offset });
    }

    let default_sample_size = read_u32(body, offset)?;
    let sample_count = read_u32(&body[4..], offset)?;

    let sample_sizes = if default_sample_size == 0 {
        let sizes_data = &body[8..];
        if sizes_data.len() < sample_count as usize * 4 {
            return Err(Mp4Error::UnexpectedEof { offset });
        }
        let mut sizes = Vec::with_capacity(sample_count as usize);
        for i in 0..sample_count as usize {
            sizes.push(read_u32(&sizes_data[i * 4..], offset)?);
        }
        sizes
    } else {
        Vec::new()
    };

    Ok(SampleSizeBox {
        default_sample_size,
        sample_sizes,
        sample_count,
    })
}
