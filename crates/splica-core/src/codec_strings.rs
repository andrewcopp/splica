//! WebCodecs codec string builders and audio frame duration helpers.
//!
//! Builds codec strings from raw container-level config records (avcC, hvcC,
//! av1C, VP9 CodecPrivate). These are used by WASM bindings across container
//! crates to create `VideoDecoderConfig` / `AudioDecoderConfig` objects.

use crate::media::{AudioCodec, Codec, TrackInfo, TrackKind};

/// Builds a WebCodecs AVC codec string from avcC data.
///
/// Format: `avc1.PPCCLL` where PP=profile, CC=compatibility, LL=level.
/// Falls back to `"avc1"` if the avcC data is too short.
pub fn build_avc_codec_string(avcc: &[u8]) -> String {
    // avcC layout: [0]=version, [1]=profile, [2]=compatibility, [3]=level
    if avcc.len() >= 4 {
        format!("avc1.{:02x}{:02x}{:02x}", avcc[1], avcc[2], avcc[3])
    } else {
        "avc1".to_string()
    }
}

/// Builds a WebCodecs AVC codec string from optional CodecPrivate data.
///
/// Convenience wrapper for container formats (e.g., MKV) where CodecPrivate
/// may be absent. Falls back to `"avc1"` when data is `None` or too short.
pub fn build_avc_codec_string_from_optional(codec_private: Option<&[u8]>) -> String {
    match codec_private {
        Some(data) => build_avc_codec_string(data),
        None => "avc1".to_string(),
    }
}

/// Builds a WebCodecs HEVC codec string from hvcC data.
///
/// Format: `hev1.{profile}.{compat_reversed}.{tier}{level}[.constraints]`
/// per ISO/IEC 14496-15. Falls back to `"hev1"` if the hvcC data is too short.
pub fn build_hevc_codec_string(hvcc: &[u8]) -> String {
    // hvcC layout (from ISO 14496-15 S8.3.3.1.2):
    //   [0]    = configurationVersion
    //   [1]    = general_profile_space(2) | general_tier_flag(1) | general_profile_idc(5)
    //   [2..6] = general_profile_compatibility_flags (4 bytes, big-endian)
    //   [6..12]= general_constraint_indicator_flags (6 bytes)
    //   [12]   = general_level_idc
    if hvcc.len() < 13 {
        return "hev1".to_string();
    }

    let profile_idc = hvcc[1] & 0x1F;
    let tier_flag = (hvcc[1] >> 5) & 0x01;
    let tier = if tier_flag == 1 { 'H' } else { 'L' };
    let level_idc = hvcc[12];

    // Profile compatibility as a reversed 32-bit hex value
    let compat = u32::from_be_bytes([hvcc[2], hvcc[3], hvcc[4], hvcc[5]]);
    let compat_reversed = compat.reverse_bits();

    // Constraint bytes -- encode as dot-separated hex, trimming trailing zeros
    let constraints = &hvcc[6..12];
    let last_nonzero = constraints
        .iter()
        .rposition(|&b| b != 0)
        .map(|i| i + 1)
        .unwrap_or(0);

    let mut result = format!(
        "hev1.{}.{:X}.{}{}",
        profile_idc, compat_reversed, tier, level_idc
    );
    for &byte in &constraints[..last_nonzero] {
        result.push_str(&format!(".{:02X}", byte));
    }
    result
}

/// Builds a WebCodecs AV1 codec string from av1C data.
///
/// Format: `av01.P.LLT.BB` per AV1 codec ISO media file format.
/// Falls back to `"av01"` if the av1C data is too short.
pub fn build_av1_codec_string(av1c: &[u8]) -> String {
    // av1C layout (AV1 Codec ISO Media File Format S2.3.1):
    //   [0] = marker(1) | version(7)
    //   [1] = seq_profile(3) | seq_level_idx_0(5)
    //   [2] = seq_tier_0(1) | high_bitdepth(1) | twelve_bit(1) | ...
    if av1c.len() < 4 {
        return "av01".to_string();
    }

    let profile = (av1c[1] >> 5) & 0x07;
    let level = av1c[1] & 0x1F;
    let tier = if (av1c[2] >> 7) & 0x01 == 1 { 'H' } else { 'M' };

    let high_bitdepth = (av1c[2] >> 6) & 0x01;
    let twelve_bit = (av1c[2] >> 5) & 0x01;
    let bit_depth = if high_bitdepth == 1 {
        if twelve_bit == 1 {
            12
        } else {
            10
        }
    } else {
        8
    };

    format!("av01.{profile}.{level:02}{tier}.{bit_depth:02}")
}

/// Builds a WebCodecs VP9 codec string from CodecPrivate data.
///
/// Parses VP Codec ISO Media File Format features (profile, level, bit depth)
/// from the CodecPrivate bytes. Falls back to `"vp09.00.10.08"` (profile 0,
/// level 1.0, 8-bit) when CodecPrivate is absent or too short to parse.
pub fn build_vp9_codec_string(codec_private: Option<&[u8]>) -> String {
    let mut profile: u8 = 0;
    let mut level: u8 = 10;
    let mut bit_depth: u8 = 8;

    if let Some(data) = codec_private {
        // VP Codec ISO Media File Format: sequence of (id: u8, length: u8, value: [u8])
        let mut pos = 0;
        while pos + 2 <= data.len() {
            let id = data[pos];
            let len = data[pos + 1] as usize;
            pos += 2;
            if pos + len > data.len() {
                break;
            }
            if len == 1 {
                match id {
                    1 => profile = data[pos],
                    2 => level = data[pos],
                    3 => bit_depth = data[pos],
                    _ => {}
                }
            }
            pos += len;
        }
    }

    format!("vp09.{profile:02}.{level:02}.{bit_depth:02}")
}

/// Extracts the AAC Audio Object Type from esds box data.
///
/// The esds box contains an ES_Descriptor with a DecoderConfigDescriptor
/// that holds an AudioSpecificConfig. The first 5 bits of the
/// AudioSpecificConfig encode the Audio Object Type:
/// - 2 = AAC-LC (most common, codec string "mp4a.40.2")
/// - 5 = SBR / HE-AAC (codec string "mp4a.40.5")
/// - 29 = PS / HE-AAC v2 (codec string "mp4a.40.29")
///
/// Falls back to 2 (AAC-LC) if the esds data is too short to parse.
pub fn extract_aac_audio_object_type(esds: &[u8]) -> u8 {
    // Walk the esds looking for DecoderSpecificInfo (tag 0x05)
    // The esds structure is:
    //   version(4) + ES_Descriptor(tag 0x03) {
    //     ES_ID(2) + flags(1) + DecoderConfigDescriptor(tag 0x04) {
    //       objectTypeIndication(1) + streamType(1) + bufferSizeDB(3) +
    //       maxBitrate(4) + avgBitrate(4) + DecoderSpecificInfo(tag 0x05) {
    //         AudioSpecificConfig bytes...
    //       }
    //     }
    //   }
    let mut pos = 0;

    // Skip esds full box header (version + flags = 4 bytes) if present
    if esds.len() >= 4 {
        pos = 4;
    }

    // Search for tag 0x05 (DecoderSpecificInfo)
    while pos < esds.len() {
        let tag = esds[pos];
        pos += 1;

        // Read variable-length size (1-4 bytes, each with MSB continuation flag)
        let mut size: usize = 0;
        for _ in 0..4 {
            if pos >= esds.len() {
                return 2;
            }
            let b = esds[pos];
            pos += 1;
            size = (size << 7) | (b & 0x7F) as usize;
            if b & 0x80 == 0 {
                break;
            }
        }

        if tag == 0x05 {
            // Found DecoderSpecificInfo -- first 5 bits are audioObjectType
            if pos < esds.len() {
                let aot = esds[pos] >> 3;
                if aot > 0 {
                    return aot;
                }
            }
            return 2;
        }

        // For container tags (0x03, 0x04), skip only their header fields
        // and continue scanning their children
        if tag == 0x03 {
            // ES_Descriptor: skip ES_ID(2) + flags(1) = 3 bytes
            if pos + 3 <= esds.len() {
                pos += 3;
            }
        } else if tag == 0x04 {
            // DecoderConfigDescriptor: skip fixed fields = 13 bytes
            if pos + 13 <= esds.len() {
                pos += 13;
            }
        } else {
            // Unknown tag -- skip its body entirely
            pos += size;
        }
    }

    // Default to AAC-LC
    2
}

/// Computes the audio frame duration in microseconds from a list of tracks.
///
/// Finds the first audio track and returns the per-frame duration based on
/// codec type:
/// - AAC: `1024.0 / sample_rate * 1_000_000.0`
/// - Opus: `20_000.0` (standard 20ms frame)
/// - Unknown/absent: `-1.0`
pub fn compute_audio_frame_duration(tracks: &[TrackInfo]) -> f64 {
    let track = tracks.iter().find(|t| t.kind == TrackKind::Audio);

    let track = match track {
        Some(t) => t,
        None => return -1.0,
    };

    let sample_rate = track.audio.as_ref().map(|a| a.sample_rate).unwrap_or(0);

    match &track.codec {
        Codec::Audio(AudioCodec::Aac) => 1024.0 / f64::from(sample_rate) * 1_000_000.0,
        Codec::Audio(AudioCodec::Opus) => 20_000.0,
        _ => -1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- AVC ---

    #[test]
    fn test_that_avc_codec_string_parses_baseline_profile() {
        // version=1, profile=66 (Baseline), compat=0xC0, level=30
        let avcc = [0x01, 0x42, 0xC0, 0x1E];

        assert_eq!(build_avc_codec_string(&avcc), "avc1.42c01e");
    }

    #[test]
    fn test_that_avc_codec_string_parses_high_profile() {
        // version=1, profile=100 (High), compat=0x00, level=31
        let avcc = [0x01, 0x64, 0x00, 0x1F];

        assert_eq!(build_avc_codec_string(&avcc), "avc1.64001f");
    }

    #[test]
    fn test_that_avc_codec_string_falls_back_on_truncated_data() {
        assert_eq!(build_avc_codec_string(&[0x01, 0x42]), "avc1");
    }

    #[test]
    fn test_that_avc_codec_string_falls_back_on_empty_data() {
        assert_eq!(build_avc_codec_string(&[]), "avc1");
    }

    #[test]
    fn test_that_avc_optional_falls_back_on_none() {
        assert_eq!(build_avc_codec_string_from_optional(None), "avc1");
    }

    #[test]
    fn test_that_avc_optional_parses_valid_data() {
        let avcc = [0x01, 0x42, 0xC0, 0x1E];

        assert_eq!(
            build_avc_codec_string_from_optional(Some(&avcc)),
            "avc1.42c01e"
        );
    }

    // --- HEVC ---

    #[test]
    fn test_that_hevc_codec_string_parses_main_profile_level_93() {
        let hvcc = [
            0x01, // [0] configurationVersion
            0x01, // [1] profile_space=0, tier=0, profile_idc=1
            0x60, 0x00, 0x00, 0x00, // [2..6] profile_compatibility
            0x90, 0x00, 0x00, 0x00, 0x00, 0x00, // [6..12] constraint_indicator
            93,   // [12] level_idc
        ];

        assert_eq!(build_hevc_codec_string(&hvcc), "hev1.1.6.L93.90");
    }

    #[test]
    fn test_that_hevc_codec_string_handles_high_tier() {
        let mut hvcc = [0u8; 13];
        hvcc[1] = 0b0010_0001; // tier=1, profile_idc=1
        hvcc[2..6].copy_from_slice(&[0x60, 0x00, 0x00, 0x00]);
        hvcc[12] = 150; // level 5.0

        let result = build_hevc_codec_string(&hvcc);
        assert!(result.starts_with("hev1.1.6.H150"));
    }

    #[test]
    fn test_that_hevc_codec_string_falls_back_on_truncated_data() {
        assert_eq!(build_hevc_codec_string(&[0x01; 5]), "hev1");
    }

    #[test]
    fn test_that_hevc_codec_string_omits_trailing_zero_constraints() {
        let mut hvcc = [0u8; 13];
        hvcc[1] = 0x01; // profile_idc=1
        hvcc[2..6].copy_from_slice(&[0x60, 0x00, 0x00, 0x00]);
        hvcc[12] = 93;

        assert_eq!(build_hevc_codec_string(&hvcc), "hev1.1.6.L93");
    }

    // --- AV1 ---

    #[test]
    fn test_that_av1_codec_string_parses_main_profile_8bit() {
        let av1c = [0x81, 0x08, 0x00, 0x00];

        assert_eq!(build_av1_codec_string(&av1c), "av01.0.08M.08");
    }

    #[test]
    fn test_that_av1_codec_string_parses_high_profile_10bit() {
        let av1c = [0x81, 0x2D, 0x40, 0x00];

        assert_eq!(build_av1_codec_string(&av1c), "av01.1.13M.10");
    }

    #[test]
    fn test_that_av1_codec_string_parses_professional_12bit_high_tier() {
        let av1c = [0x81, 0x53, 0xE0, 0x00];

        assert_eq!(build_av1_codec_string(&av1c), "av01.2.19H.12");
    }

    #[test]
    fn test_that_av1_codec_string_falls_back_on_truncated_data() {
        assert_eq!(build_av1_codec_string(&[0x81]), "av01");
    }

    // --- VP9 ---

    #[test]
    fn test_that_vp9_codec_string_defaults_without_codec_private() {
        assert_eq!(build_vp9_codec_string(None), "vp09.00.10.08");
    }

    #[test]
    fn test_that_vp9_codec_string_parses_profile_level_bitdepth() {
        // id=1 (profile), len=1, value=2
        // id=2 (level), len=1, value=31
        // id=3 (bit_depth), len=1, value=10
        let data = [1, 1, 2, 2, 1, 31, 3, 1, 10];

        assert_eq!(build_vp9_codec_string(Some(&data)), "vp09.02.31.10");
    }

    // --- AAC ---

    #[test]
    fn test_that_aac_lc_object_type_is_extracted_from_esds() {
        let esds: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, // version + flags
            0x03, 0x19, // ES_Descriptor, size=25
            0x00, 0x01, // ES_ID
            0x00, // flags
            0x04, 0x11, // DecoderConfigDescriptor, size=17
            0x40, 0x15, 0x00, 0x00, 0x00, // objectType, streamType, bufferSize
            0x00, 0x01, 0xF4, 0x00, // maxBitrate
            0x00, 0x01, 0xF4, 0x00, // avgBitrate
            0x05, 0x02, // DecoderSpecificInfo, size=2
            0x11, 0x90, // AudioSpecificConfig: AOT=2(AAC-LC)
        ];

        assert_eq!(extract_aac_audio_object_type(esds), 2);
    }

    #[test]
    fn test_that_he_aac_object_type_is_extracted_from_esds() {
        let esds: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, // version + flags
            0x03, 0x19, // ES_Descriptor, size=25
            0x00, 0x01, // ES_ID
            0x00, // flags
            0x04, 0x11, // DecoderConfigDescriptor, size=17
            0x40, 0x15, 0x00, 0x00, 0x00, // objectType, streamType, bufferSize
            0x00, 0x01, 0xF4, 0x00, // maxBitrate
            0x00, 0x01, 0xF4, 0x00, // avgBitrate
            0x05, 0x02, // DecoderSpecificInfo, size=2
            0x2B, 0x90, // AudioSpecificConfig: AOT=5 (HE-AAC)
        ];

        assert_eq!(extract_aac_audio_object_type(esds), 5);
    }

    #[test]
    fn test_that_empty_esds_defaults_to_aac_lc() {
        assert_eq!(extract_aac_audio_object_type(&[]), 2);
    }

    #[test]
    fn test_that_truncated_esds_defaults_to_aac_lc() {
        assert_eq!(extract_aac_audio_object_type(&[0x00, 0x00, 0x00, 0x00]), 2);
    }
}
