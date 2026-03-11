//! Minimal H.264 SPS (Sequence Parameter Set) parser.
//!
//! Parses only the fields needed to extract VUI color parameters.
//! Does not attempt to parse the full SPS — only enough to reach
//! `vui_parameters_present_flag` and the color description fields.

use splica_core::media::{
    ColorPrimaries, ColorRange, ColorSpace, MatrixCoefficients, TransferCharacteristics,
};

/// Color parameters extracted from the SPS VUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpsColorInfo {
    pub color_space: ColorSpace,
}

/// Parses color info from a raw SPS NAL unit (without start code or length prefix).
///
/// Returns `None` if:
/// - The SPS is too short or malformed
/// - VUI parameters are not present
/// - The colour_description_present_flag is not set
/// - The color values don't map to known splica types
pub(crate) fn parse_sps_color_info(sps_nalu: &[u8]) -> Option<SpsColorInfo> {
    // Skip the NAL header byte (forbidden_zero_bit + nal_ref_idc + nal_unit_type)
    if sps_nalu.is_empty() {
        return None;
    }
    let mut reader = BitReader::new(&sps_nalu[1..]);

    // profile_idc (8 bits)
    let profile_idc = reader.read_bits(8)?;
    // constraint_set0..5_flags + reserved_zero_2bits (8 bits)
    reader.read_bits(8)?;
    // level_idc (8 bits)
    reader.read_bits(8)?;
    // seq_parameter_set_id (ue(v))
    reader.read_exp_golomb()?;

    // High profiles have extra fields before the common SPS body
    let high_profiles = [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135];
    if high_profiles.contains(&profile_idc) {
        let chroma_format_idc = reader.read_exp_golomb()?;
        if chroma_format_idc == 3 {
            // separate_colour_plane_flag
            reader.read_bits(1)?;
        }
        // bit_depth_luma_minus8
        reader.read_exp_golomb()?;
        // bit_depth_chroma_minus8
        reader.read_exp_golomb()?;
        // qpprime_y_zero_transform_bypass_flag
        reader.read_bits(1)?;
        // seq_scaling_matrix_present_flag
        let scaling_present = reader.read_bits(1)?;
        if scaling_present == 1 {
            let count = if chroma_format_idc != 3 { 8 } else { 12 };
            for _ in 0..count {
                let list_present = reader.read_bits(1)?;
                if list_present == 1 {
                    let size = if count <= 6 { 16 } else { 64 };
                    skip_scaling_list(&mut reader, size)?;
                }
            }
        }
    }

    // log2_max_frame_num_minus4
    reader.read_exp_golomb()?;
    // pic_order_cnt_type
    let poc_type = reader.read_exp_golomb()?;
    if poc_type == 0 {
        // log2_max_pic_order_cnt_lsb_minus4
        reader.read_exp_golomb()?;
    } else if poc_type == 1 {
        // delta_pic_order_always_zero_flag
        reader.read_bits(1)?;
        // offset_for_non_ref_pic (se(v))
        reader.read_signed_exp_golomb()?;
        // offset_for_top_to_bottom_field (se(v))
        reader.read_signed_exp_golomb()?;
        let num_ref_frames_in_poc_cycle = reader.read_exp_golomb()?;
        for _ in 0..num_ref_frames_in_poc_cycle {
            reader.read_signed_exp_golomb()?;
        }
    }

    // max_num_ref_frames
    reader.read_exp_golomb()?;
    // gaps_in_frame_num_value_allowed_flag
    reader.read_bits(1)?;
    // pic_width_in_mbs_minus1
    reader.read_exp_golomb()?;
    // pic_height_in_map_units_minus1
    reader.read_exp_golomb()?;
    // frame_mbs_only_flag
    let frame_mbs_only = reader.read_bits(1)?;
    if frame_mbs_only == 0 {
        // mb_adaptive_frame_field_flag
        reader.read_bits(1)?;
    }
    // direct_8x8_inference_flag
    reader.read_bits(1)?;
    // frame_cropping_flag
    let cropping = reader.read_bits(1)?;
    if cropping == 1 {
        reader.read_exp_golomb()?; // left
        reader.read_exp_golomb()?; // right
        reader.read_exp_golomb()?; // top
        reader.read_exp_golomb()?; // bottom
    }

    // vui_parameters_present_flag
    let vui_present = reader.read_bits(1)?;
    if vui_present == 0 {
        return None;
    }

    // --- VUI parameters ---
    // aspect_ratio_info_present_flag
    let aspect_ratio_present = reader.read_bits(1)?;
    if aspect_ratio_present == 1 {
        let aspect_ratio_idc = reader.read_bits(8)?;
        if aspect_ratio_idc == 255 {
            // Extended_SAR
            reader.read_bits(16)?; // sar_width
            reader.read_bits(16)?; // sar_height
        }
    }

    // overscan_info_present_flag
    let overscan_present = reader.read_bits(1)?;
    if overscan_present == 1 {
        reader.read_bits(1)?; // overscan_appropriate_flag
    }

    // video_signal_type_present_flag
    let video_signal_present = reader.read_bits(1)?;
    if video_signal_present == 0 {
        return None;
    }

    // video_format (3 bits)
    reader.read_bits(3)?;
    // video_full_range_flag (1 bit)
    let full_range_flag = reader.read_bits(1)?;
    // colour_description_present_flag
    let colour_desc_present = reader.read_bits(1)?;
    if colour_desc_present == 0 {
        return None;
    }

    // colour_primaries (8 bits)
    let colour_primaries = reader.read_bits(8)?;
    // transfer_characteristics (8 bits)
    let transfer = reader.read_bits(8)?;
    // matrix_coefficients (8 bits)
    let matrix = reader.read_bits(8)?;

    let primaries = map_color_primaries(colour_primaries as u8)?;
    let transfer = map_transfer_characteristics(transfer as u8)?;
    let matrix = map_matrix_coefficients(matrix as u8)?;
    let range = if full_range_flag == 1 {
        ColorRange::Full
    } else {
        ColorRange::Limited
    };

    Some(SpsColorInfo {
        color_space: ColorSpace {
            primaries,
            transfer,
            matrix,
            range,
        },
    })
}

fn map_color_primaries(val: u8) -> Option<ColorPrimaries> {
    match val {
        1 => Some(ColorPrimaries::Bt709),
        9 => Some(ColorPrimaries::Bt2020),
        12 => Some(ColorPrimaries::Smpte432),
        _ => None,
    }
}

fn map_transfer_characteristics(val: u8) -> Option<TransferCharacteristics> {
    match val {
        1 => Some(TransferCharacteristics::Bt709),
        16 => Some(TransferCharacteristics::Smpte2084),
        18 => Some(TransferCharacteristics::HybridLogGamma),
        _ => None,
    }
}

fn map_matrix_coefficients(val: u8) -> Option<MatrixCoefficients> {
    match val {
        0 => Some(MatrixCoefficients::Identity),
        1 => Some(MatrixCoefficients::Bt709),
        9 => Some(MatrixCoefficients::Bt2020NonConstant),
        10 => Some(MatrixCoefficients::Bt2020Constant),
        _ => None,
    }
}

fn skip_scaling_list(reader: &mut BitReader<'_>, size: u32) -> Option<()> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _ in 0..size {
        if next_scale != 0 {
            let delta = reader.read_signed_exp_golomb()?;
            next_scale = (last_scale + delta as i32 + 256) % 256;
        }
        last_scale = if next_scale == 0 {
            last_scale
        } else {
            next_scale
        };
    }
    Some(())
}

/// A simple bitstream reader for parsing H.264 NAL units.
struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bits(&mut self, count: u8) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            if self.byte_pos >= self.data.len() {
                return None;
            }
            let bit = (self.data[self.byte_pos] >> (7 - self.bit_pos)) & 1;
            value = (value << 1) | bit as u32;
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
        }
        Some(value)
    }

    /// Reads an unsigned Exp-Golomb coded value (ue(v)).
    fn read_exp_golomb(&mut self) -> Option<u32> {
        let mut leading_zeros = 0u32;
        loop {
            let bit = self.read_bits(1)?;
            if bit == 1 {
                break;
            }
            leading_zeros += 1;
            if leading_zeros > 31 {
                return None;
            }
        }
        if leading_zeros == 0 {
            return Some(0);
        }
        let suffix = self.read_bits(leading_zeros as u8)?;
        Some((1 << leading_zeros) - 1 + suffix)
    }

    /// Reads a signed Exp-Golomb coded value (se(v)).
    fn read_signed_exp_golomb(&mut self) -> Option<i64> {
        let code = self.read_exp_golomb()?;
        let value = if code % 2 == 0 {
            -(code as i64 / 2)
        } else {
            (code as i64 + 1) / 2
        };
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_bt709_sps_parses_color_info() {
        // GIVEN — a real-world SPS NAL unit with BT.709 color info
        // This is a Baseline profile SPS with VUI that signals BT.709
        let sps = build_test_sps(
            66, // Baseline profile
            30, // Level 3.0
            1,  // BT.709 primaries
            1,  // BT.709 transfer
            1,  // BT.709 matrix
            0,  // Limited range
        );

        // WHEN
        let info = parse_sps_color_info(&sps);

        // THEN
        let info = info.unwrap();
        assert_eq!(info.color_space.primaries, ColorPrimaries::Bt709);
        assert_eq!(info.color_space.transfer, TransferCharacteristics::Bt709);
        assert_eq!(info.color_space.matrix, MatrixCoefficients::Bt709);
        assert_eq!(info.color_space.range, ColorRange::Limited);
    }

    #[test]
    fn test_that_bt2020_pq_sps_parses_color_info() {
        // GIVEN — an SPS with BT.2020 + PQ (HDR10)
        let sps = build_test_sps(
            100, // High profile
            51,  // Level 5.1
            9,   // BT.2020 primaries
            16,  // SMPTE ST 2084 (PQ)
            9,   // BT.2020 non-constant matrix
            0,   // Limited range
        );

        // WHEN
        let info = parse_sps_color_info(&sps);

        // THEN
        let info = info.unwrap();
        assert_eq!(info.color_space.primaries, ColorPrimaries::Bt2020);
        assert_eq!(
            info.color_space.transfer,
            TransferCharacteristics::Smpte2084
        );
        assert_eq!(
            info.color_space.matrix,
            MatrixCoefficients::Bt2020NonConstant
        );
    }

    #[test]
    fn test_that_sps_without_vui_returns_none() {
        // GIVEN — a minimal SPS without VUI
        let sps = build_test_sps_no_vui(66, 30);

        // WHEN
        let info = parse_sps_color_info(&sps);

        // THEN
        assert!(info.is_none());
    }

    #[test]
    fn test_that_sps_with_unknown_primaries_returns_none() {
        // GIVEN — SPS with BT.601 primaries (value 6, not in our enum)
        let sps = build_test_sps(
            66, // Baseline
            30, 6, // SMPTE 170M (BT.601)
            6, // BT.601 transfer
            6, // BT.601 matrix
            0,
        );

        // WHEN
        let info = parse_sps_color_info(&sps);

        // THEN — we can't represent BT.601, so None
        assert!(info.is_none());
    }

    #[test]
    fn test_that_full_range_flag_is_parsed() {
        // GIVEN — BT.709 with full range
        let sps = build_test_sps(66, 30, 1, 1, 1, 1);

        // WHEN
        let info = parse_sps_color_info(&sps).unwrap();

        // THEN
        assert_eq!(info.color_space.range, ColorRange::Full);
    }

    /// Builds a minimal SPS NAL unit with VUI color parameters.
    fn build_test_sps(
        profile: u8,
        level: u8,
        primaries: u8,
        transfer: u8,
        matrix: u8,
        full_range: u8,
    ) -> Vec<u8> {
        let mut writer = BitWriter::new();

        // NAL header: forbidden(0) + nal_ref_idc(3) + nal_unit_type(7=SPS)
        writer.write_byte(0x67);

        // profile_idc
        writer.write_byte(profile);
        // constraint_set flags + reserved
        writer.write_byte(0x00);
        // level_idc
        writer.write_byte(level);
        // seq_parameter_set_id = 0 (ue: 1)
        writer.write_exp_golomb(0);

        if [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135].contains(&profile) {
            // chroma_format_idc = 1 (4:2:0)
            writer.write_exp_golomb(1);
            // bit_depth_luma_minus8 = 0
            writer.write_exp_golomb(0);
            // bit_depth_chroma_minus8 = 0
            writer.write_exp_golomb(0);
            // qpprime_y_zero_transform_bypass_flag = 0
            writer.write_bit(0);
            // seq_scaling_matrix_present_flag = 0
            writer.write_bit(0);
        }

        // log2_max_frame_num_minus4 = 0
        writer.write_exp_golomb(0);
        // pic_order_cnt_type = 0
        writer.write_exp_golomb(0);
        // log2_max_pic_order_cnt_lsb_minus4 = 0
        writer.write_exp_golomb(0);
        // max_num_ref_frames = 1
        writer.write_exp_golomb(1);
        // gaps_in_frame_num_value_allowed_flag = 0
        writer.write_bit(0);
        // pic_width_in_mbs_minus1 = 119 (1920/16 - 1)
        writer.write_exp_golomb(119);
        // pic_height_in_map_units_minus1 = 67 (1088/16 - 1)
        writer.write_exp_golomb(67);
        // frame_mbs_only_flag = 1
        writer.write_bit(1);
        // direct_8x8_inference_flag = 1
        writer.write_bit(1);
        // frame_cropping_flag = 0
        writer.write_bit(0);

        // vui_parameters_present_flag = 1
        writer.write_bit(1);

        // --- VUI ---
        // aspect_ratio_info_present_flag = 0
        writer.write_bit(0);
        // overscan_info_present_flag = 0
        writer.write_bit(0);
        // video_signal_type_present_flag = 1
        writer.write_bit(1);
        // video_format = 5 (unspecified)
        writer.write_bits(5, 3);
        // video_full_range_flag
        writer.write_bit(full_range);
        // colour_description_present_flag = 1
        writer.write_bit(1);
        // colour_primaries
        writer.write_byte(primaries);
        // transfer_characteristics
        writer.write_byte(transfer);
        // matrix_coefficients
        writer.write_byte(matrix);

        writer.finish()
    }

    /// Builds a minimal SPS NAL unit without VUI parameters.
    fn build_test_sps_no_vui(profile: u8, level: u8) -> Vec<u8> {
        let mut writer = BitWriter::new();

        writer.write_byte(0x67);
        writer.write_byte(profile);
        writer.write_byte(0x00);
        writer.write_byte(level);
        writer.write_exp_golomb(0); // seq_parameter_set_id
        writer.write_exp_golomb(0); // log2_max_frame_num_minus4
        writer.write_exp_golomb(0); // pic_order_cnt_type
        writer.write_exp_golomb(0); // log2_max_pic_order_cnt_lsb_minus4
        writer.write_exp_golomb(1); // max_num_ref_frames
        writer.write_bit(0); // gaps_in_frame_num
        writer.write_exp_golomb(119); // width
        writer.write_exp_golomb(67); // height
        writer.write_bit(1); // frame_mbs_only
        writer.write_bit(1); // direct_8x8
        writer.write_bit(0); // cropping
        writer.write_bit(0); // vui_parameters_present_flag = 0

        writer.finish()
    }

    /// Bit-level writer for constructing test SPS NAL units.
    struct BitWriter {
        data: Vec<u8>,
        current_byte: u8,
        bit_pos: u8,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                data: Vec::new(),
                current_byte: 0,
                bit_pos: 0,
            }
        }

        fn write_bit(&mut self, bit: u8) {
            self.current_byte |= (bit & 1) << (7 - self.bit_pos);
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.data.push(self.current_byte);
                self.current_byte = 0;
                self.bit_pos = 0;
            }
        }

        fn write_bits(&mut self, value: u32, count: u8) {
            for i in (0..count).rev() {
                self.write_bit(((value >> i) & 1) as u8);
            }
        }

        fn write_byte(&mut self, byte: u8) {
            self.write_bits(byte as u32, 8);
        }

        fn write_exp_golomb(&mut self, value: u32) {
            if value == 0 {
                self.write_bit(1);
            } else {
                let code = value + 1;
                let bits = 32 - code.leading_zeros();
                // Write (bits-1) leading zeros
                for _ in 0..bits - 1 {
                    self.write_bit(0);
                }
                // Write the code
                self.write_bits(code, bits as u8);
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.bit_pos > 0 {
                self.data.push(self.current_byte);
            }
            self.data
        }
    }
}
