//! Box-building helpers specific to fragmented MP4 (fMP4).
//!
//! These functions produce raw byte vectors for ISO BMFF boxes that are unique
//! to the fragmented MP4 format (moof/mdat fragments, trex, tfhd, tfdt, trun,
//! etc.). Shared box helpers (mvhd, tkhd, etc.) live in [`crate::box_builders`].

use splica_core::MuxError;

use crate::box_builders::{make_box, make_full_box};

/// A sample buffered within a fragment before it is flushed.
pub(crate) struct FragmentSample {
    pub(crate) data: Vec<u8>,
    pub(crate) dts_ticks: i64,
    pub(crate) cts_offset: i32,
    pub(crate) is_keyframe: bool,
}

/// ftyp for fragmented MP4 — uses iso5/iso6/msdh brands.
pub(crate) fn build_fmp4_ftyp() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"iso5"); // major brand
    body.extend_from_slice(&1u32.to_be_bytes()); // minor version
    body.extend_from_slice(b"iso5"); // compatible brands
    body.extend_from_slice(b"iso6");
    body.extend_from_slice(b"msdh");
    body.extend_from_slice(b"msix");
    make_box(b"ftyp", &body)
}

/// Empty stts box for the init segment.
pub(crate) fn build_empty_stts() -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&0u32.to_be_bytes()); // entry_count = 0
    make_full_box(b"stts", &body)
}

/// Empty stsc box for the init segment.
pub(crate) fn build_empty_stsc() -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0];
    body.extend_from_slice(&0u32.to_be_bytes());
    make_full_box(b"stsc", &body)
}

/// Empty stsz box for the init segment.
pub(crate) fn build_empty_stsz() -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0];
    body.extend_from_slice(&0u32.to_be_bytes()); // default_sample_size
    body.extend_from_slice(&0u32.to_be_bytes()); // sample_count
    make_full_box(b"stsz", &body)
}

/// Empty stco box for the init segment.
pub(crate) fn build_empty_stco() -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0];
    body.extend_from_slice(&0u32.to_be_bytes()); // entry_count
    make_full_box(b"stco", &body)
}

/// trex (Track Extends) — default values for track fragments.
pub(crate) fn build_trex(track_id: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&track_id.to_be_bytes());
    body.extend_from_slice(&1u32.to_be_bytes()); // default_sample_description_index
    body.extend_from_slice(&0u32.to_be_bytes()); // default_sample_duration
    body.extend_from_slice(&0u32.to_be_bytes()); // default_sample_size
    body.extend_from_slice(&0u32.to_be_bytes()); // default_sample_flags
    make_full_box(b"trex", &body)
}

/// mfhd (Movie Fragment Header).
pub(crate) fn build_mfhd(sequence_number: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&sequence_number.to_be_bytes());
    make_full_box(b"mfhd", &body)
}

/// Build a traf box for one track's fragment.
pub(crate) fn build_traf(
    track_id: u32,
    base_decode_time: u64,
    pending_samples: &[FragmentSample],
    data_offset: u32,
) -> Result<Vec<u8>, MuxError> {
    let tfhd = build_tfhd(track_id);
    let tfdt = build_tfdt(base_decode_time);
    let trun = build_trun(pending_samples, data_offset);

    let mut traf_body = tfhd;
    traf_body.extend_from_slice(&tfdt);
    traf_body.extend_from_slice(&trun);
    Ok(make_box(b"traf", &traf_body))
}

/// tfhd (Track Fragment Header).
///
/// Uses flag 0x020000 (default-base-is-moof) so data_offset in trun
/// is relative to the moof box start.
pub(crate) fn build_tfhd(track_id: u32) -> Vec<u8> {
    // version=0, flags=0x020000 (default-base-is-moof)
    let mut body = vec![0u8, 0x02, 0x00, 0x00];
    body.extend_from_slice(&track_id.to_be_bytes());
    make_full_box(b"tfhd", &body)
}

/// tfdt (Track Fragment Decode Time) — version 1 for 64-bit time.
pub(crate) fn build_tfdt(base_media_decode_time: u64) -> Vec<u8> {
    let mut body = Vec::new();
    // version=1, flags=0
    body.extend_from_slice(&[1, 0, 0, 0]);
    body.extend_from_slice(&base_media_decode_time.to_be_bytes());
    make_full_box(b"tfdt", &body)
}

/// trun (Track Fragment Run).
///
/// Flags: 0x000001 (data_offset_present)
///      | 0x000100 (sample_duration_present)
///      | 0x000200 (sample_size_present)
///      | 0x000400 (sample_flags_present)
///      | 0x000800 (sample_composition_time_offsets_present)
pub(crate) fn build_trun(samples: &[FragmentSample], data_offset: u32) -> Vec<u8> {
    // Determine if we need composition time offsets
    let has_cts = samples.iter().any(|s| s.cts_offset != 0);

    let mut flags: u32 = 0x000001 // data_offset_present
        | 0x000100  // sample_duration_present
        | 0x000200  // sample_size_present
        | 0x000400; // sample_flags_present
    if has_cts {
        flags |= 0x000800; // sample_composition_time_offsets_present
    }

    let mut body = Vec::new();
    // version=0 (version=1 needed for signed CTS offsets, but we use 0 for compatibility)
    let version: u8 = if has_cts { 1 } else { 0 };
    body.push(version);
    body.push((flags >> 16) as u8);
    body.push((flags >> 8) as u8);
    body.push(flags as u8);

    // sample_count
    body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    // data_offset (signed i32)
    body.extend_from_slice(&(data_offset as i32).to_be_bytes());

    // Per-sample entries
    for (i, sample) in samples.iter().enumerate() {
        // sample_duration: delta to next sample, or repeat last
        let duration = if i + 1 < samples.len() {
            (samples[i + 1].dts_ticks - sample.dts_ticks) as u32
        } else if i > 0 {
            (sample.dts_ticks - samples[i - 1].dts_ticks) as u32
        } else {
            1 // single sample — use 1 as fallback duration
        };
        body.extend_from_slice(&duration.to_be_bytes());

        // sample_size
        body.extend_from_slice(&(sample.data.len() as u32).to_be_bytes());

        // sample_flags
        let flags = if sample.is_keyframe {
            0x02000000u32 // sample_depends_on=2 (does not depend on others)
        } else {
            0x01010000u32 // sample_depends_on=1 (depends on others), sample_is_non_sync=1
        };
        body.extend_from_slice(&flags.to_be_bytes());

        // sample_composition_time_offset (if present)
        if has_cts {
            body.extend_from_slice(&(sample.cts_offset).to_be_bytes());
        }
    }

    make_full_box(b"trun", &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_fmp4_ftyp_starts_with_correct_major_brand() {
        let ftyp = build_fmp4_ftyp();

        // Skip 8-byte box header (size + "ftyp"), major brand is next 4 bytes
        assert_eq!(&ftyp[8..12], b"iso5");
    }

    #[test]
    fn test_that_trex_embeds_track_id() {
        let trex = build_trex(42);

        // Box header (8) + version/flags (4) = offset 12 for track_id
        let track_id = u32::from_be_bytes([trex[12], trex[13], trex[14], trex[15]]);
        assert_eq!(track_id, 42);
    }

    #[test]
    fn test_that_mfhd_embeds_sequence_number() {
        let mfhd = build_mfhd(7);

        // Box header (8) + version/flags (4) = offset 12 for sequence_number
        let seq = u32::from_be_bytes([mfhd[12], mfhd[13], mfhd[14], mfhd[15]]);
        assert_eq!(seq, 7);
    }

    #[test]
    fn test_that_tfdt_embeds_base_decode_time_as_64bit() {
        let tfdt = build_tfdt(123456789);

        // Box header (8) + version/flags (4) = offset 12 for base_media_decode_time
        let time = u64::from_be_bytes([
            tfdt[12], tfdt[13], tfdt[14], tfdt[15], tfdt[16], tfdt[17], tfdt[18], tfdt[19],
        ]);
        assert_eq!(time, 123456789);
    }

    #[test]
    fn test_that_tfhd_uses_default_base_is_moof_flag() {
        let tfhd = build_tfhd(1);

        // Byte 8 = version (0), bytes 9..12 = flags
        assert_eq!(tfhd[8], 0); // version
        assert_eq!(tfhd[9], 0x02); // flags byte 0
        assert_eq!(tfhd[10], 0x00); // flags byte 1
        assert_eq!(tfhd[11], 0x00); // flags byte 2
    }

    #[test]
    fn test_that_trun_single_keyframe_has_correct_sample_count() {
        let samples = vec![FragmentSample {
            data: vec![0xAA; 100],
            dts_ticks: 0,
            cts_offset: 0,
            is_keyframe: true,
        }];

        let trun = build_trun(&samples, 88);

        // Box header (8) + version/flags (4) = offset 12 for sample_count
        let count = u32::from_be_bytes([trun[12], trun[13], trun[14], trun[15]]);
        assert_eq!(count, 1);
    }
}
