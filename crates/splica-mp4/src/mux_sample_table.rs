//! Sample table box builders for the MP4 muxer.
//!
//! These functions serialize mux-time sample metadata into the binary
//! box formats required by the MP4 container (stts, ctts, stsc, stsz,
//! co64, stss, ftyp).

use crate::box_builders::{make_box, make_full_box};

/// A single sample's metadata recorded during muxing.
pub(crate) struct MuxSample {
    pub(crate) offset: u64,
    pub(crate) size: u32,
    pub(crate) dts: i64,
    pub(crate) cts_offset: i32,
    pub(crate) is_sync: bool,
}

pub(crate) fn build_ftyp() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"isom"); // major brand
    body.extend_from_slice(&0u32.to_be_bytes()); // minor version
    body.extend_from_slice(b"isom"); // compatible brands
    body.extend_from_slice(b"iso2");
    body.extend_from_slice(b"mp41");
    make_box(b"ftyp", &body)
}

pub(crate) fn build_stts(samples: &[MuxSample]) -> Vec<u8> {
    if samples.is_empty() {
        let mut body = vec![0, 0, 0, 0]; // version+flags
        body.extend_from_slice(&0u32.to_be_bytes());
        return make_full_box(b"stts", &body);
    }

    // Compute deltas and run-length encode
    let mut entries: Vec<(u32, u32)> = Vec::new(); // (count, delta)
    for i in 0..samples.len() {
        let delta = if i + 1 < samples.len() {
            (samples[i + 1].dts - samples[i].dts) as u32
        } else if let Some(last) = entries.last() {
            last.1 // repeat last delta
        } else {
            1
        };

        if let Some(last) = entries.last_mut() {
            if last.1 == delta {
                last.0 += 1;
                continue;
            }
        }
        entries.push((1, delta));
    }

    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (count, delta) in &entries {
        body.extend_from_slice(&count.to_be_bytes());
        body.extend_from_slice(&delta.to_be_bytes());
    }
    make_full_box(b"stts", &body)
}

pub(crate) fn build_ctts(samples: &[MuxSample]) -> Option<Vec<u8>> {
    // Only needed if any sample has non-zero CTS offset
    if samples.iter().all(|s| s.cts_offset == 0) {
        return None;
    }

    let mut entries: Vec<(u32, i32)> = Vec::new();
    for sample in samples {
        if let Some(last) = entries.last_mut() {
            if last.1 == sample.cts_offset {
                last.0 += 1;
                continue;
            }
        }
        entries.push((1, sample.cts_offset));
    }

    // Use version 0 with unsigned offsets
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (count, offset) in &entries {
        body.extend_from_slice(&count.to_be_bytes());
        body.extend_from_slice(&(*offset as u32).to_be_bytes());
    }
    Some(make_full_box(b"ctts", &body))
}

pub(crate) fn build_stsc(sample_count: u32) -> Vec<u8> {
    // Simple: one chunk per sample
    let mut body = vec![0, 0, 0, 0]; // version+flags
    if sample_count > 0 {
        body.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        body.extend_from_slice(&1u32.to_be_bytes()); // first_chunk
        body.extend_from_slice(&1u32.to_be_bytes()); // samples_per_chunk
        body.extend_from_slice(&1u32.to_be_bytes()); // sample_description_index
    } else {
        body.extend_from_slice(&0u32.to_be_bytes());
    }
    make_full_box(b"stsc", &body)
}

pub(crate) fn build_stsz(samples: &[MuxSample]) -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&0u32.to_be_bytes()); // sample_size (0 = variable)
    body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for sample in samples {
        body.extend_from_slice(&sample.size.to_be_bytes());
    }
    make_full_box(b"stsz", &body)
}

pub(crate) fn build_stco(samples: &[MuxSample]) -> Vec<u8> {
    // Use co64 for safety (64-bit offsets)
    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for sample in samples {
        body.extend_from_slice(&sample.offset.to_be_bytes());
    }
    make_full_box(b"co64", &body)
}

pub(crate) fn build_stss(samples: &[MuxSample]) -> Option<Vec<u8>> {
    let keyframes: Vec<u32> = samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_sync)
        .map(|(i, _)| i as u32 + 1) // 1-based
        .collect();

    // If all samples are keyframes, stss is not needed
    if keyframes.len() == samples.len() {
        return None;
    }

    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&(keyframes.len() as u32).to_be_bytes());
    for kf in &keyframes {
        body.extend_from_slice(&kf.to_be_bytes());
    }
    Some(make_full_box(b"stss", &body))
}
