//! WebCodecs codec string builders.
//!
//! Builds codec strings from raw container-level config records (avcC, hvcC,
//! av1C). These are used by WASM bindings to create `VideoDecoderConfig`
//! objects but are testable without the `wasm` feature.

/// Builds a WebCodecs AVC codec string from avcC data.
///
/// Format: `avc1.PPCCLL` where PP=profile, CC=compatibility, LL=level.
/// Falls back to `"avc1"` if the avcC data is too short.
pub(crate) fn build_avc_codec_string(avcc: &[u8]) -> String {
    // avcC layout: [0]=version, [1]=profile, [2]=compatibility, [3]=level
    if avcc.len() >= 4 {
        format!("avc1.{:02x}{:02x}{:02x}", avcc[1], avcc[2], avcc[3])
    } else {
        "avc1".to_string()
    }
}

/// Builds a WebCodecs HEVC codec string from hvcC data.
///
/// Format: `hev1.{profile}.{compat_reversed}.{tier}{level}[.constraints]`
/// per ISO/IEC 14496-15. Falls back to `"hev1"` if the hvcC data is too short.
pub(crate) fn build_hevc_codec_string(hvcc: &[u8]) -> String {
    // hvcC layout (from ISO 14496-15 §8.3.3.1.2):
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

    // Constraint bytes — encode as dot-separated hex, trimming trailing zeros
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
pub(crate) fn build_av1_codec_string(av1c: &[u8]) -> String {
    // av1C layout (AV1 Codec ISO Media File Format §2.3.1):
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

    // --- HEVC ---

    #[test]
    fn test_that_hevc_codec_string_parses_main_profile_level_93() {
        // configurationVersion=1
        // general_profile_space=0, general_tier_flag=0, general_profile_idc=1 (Main)
        //   byte[1] = 0b00_0_00001 = 0x01
        // general_profile_compatibility_flags = 0x60000000
        //   (bit 1 set = profile 1 compatible) reversed = 0x00000006
        // general_constraint_indicator_flags = [0x90, 0x00, 0x00, 0x00, 0x00, 0x00]
        // general_level_idc = 93 (level 3.1)
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
        // All constraint bytes are zero
        hvcc[12] = 93;

        // No trailing constraint bytes should appear
        assert_eq!(build_hevc_codec_string(&hvcc), "hev1.1.6.L93");
    }

    // --- AV1 ---

    #[test]
    fn test_that_av1_codec_string_parses_main_profile_8bit() {
        // marker=1, version=1 → byte[0] = 0x81
        // seq_profile=0 (Main), seq_level_idx_0=8 → byte[1] = 0b000_01000 = 0x08
        // seq_tier_0=0 (Main), high_bitdepth=0, twelve_bit=0 → byte[2] = 0x00
        // byte[3] = 0x00
        let av1c = [0x81, 0x08, 0x00, 0x00];

        assert_eq!(build_av1_codec_string(&av1c), "av01.0.08M.08");
    }

    #[test]
    fn test_that_av1_codec_string_parses_high_profile_10bit() {
        // seq_profile=1 (High), seq_level_idx_0=13 → byte[1] = 0b001_01101 = 0x2D
        // seq_tier_0=0, high_bitdepth=1, twelve_bit=0 → byte[2] = 0b0_1_0_00000 = 0x40
        let av1c = [0x81, 0x2D, 0x40, 0x00];

        assert_eq!(build_av1_codec_string(&av1c), "av01.1.13M.10");
    }

    #[test]
    fn test_that_av1_codec_string_parses_professional_12bit_high_tier() {
        // seq_profile=2 (Professional), seq_level_idx_0=19 → byte[1] = 0b010_10011 = 0x53
        // seq_tier_0=1, high_bitdepth=1, twelve_bit=1 → byte[2] = 0b1_1_1_00000 = 0xE0
        let av1c = [0x81, 0x53, 0xE0, 0x00];

        assert_eq!(build_av1_codec_string(&av1c), "av01.2.19H.12");
    }

    #[test]
    fn test_that_av1_codec_string_falls_back_on_truncated_data() {
        assert_eq!(build_av1_codec_string(&[0x81]), "av01");
    }
}
