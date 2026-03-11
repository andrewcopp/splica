//! Parser for the HEVC Decoder Configuration Record (hvcC box body).
//!
//! Extracts VPS/SPS/PPS NAL units and the NAL length size used in MP4 sample data.

use crate::error::CodecError;

/// Parsed contents of an hvcC (HEVC Decoder Configuration Record).
#[derive(Debug, Clone)]
pub struct HevcDecoderConfig {
    /// Number of bytes used for NAL unit length fields in sample data (typically 4).
    pub nal_length_size: u8,
    /// HEVC general profile IDC (e.g., 1 = Main, 2 = Main 10).
    pub general_profile_idc: u8,
    /// HEVC general level IDC (e.g., 93 = level 3.1, 120 = level 4.0).
    pub general_level_idc: u8,
    /// Video Parameter Set NAL units.
    pub vps: Vec<Vec<u8>>,
    /// Sequence Parameter Set NAL units.
    pub sps: Vec<Vec<u8>>,
    /// Picture Parameter Set NAL units.
    pub pps: Vec<Vec<u8>>,
}

impl HevcDecoderConfig {
    /// Parses an hvcC box body into its constituent parts.
    ///
    /// Layout (ISO/IEC 14496-15 §8.3.3.1.2):
    /// ```text
    /// u8   configurationVersion (must be 1)
    /// u8   general_profile_space(2) | general_tier_flag(1) | general_profile_idc(5)
    /// u32  general_profile_compatibility_flags
    /// u48  general_constraint_indicator_flags
    /// u8   general_level_idc
    /// u16  min_spatial_segmentation_idc (12 bits + 4 reserved)
    /// u8   parallelismType (2 bits + 6 reserved)
    /// u8   chroma_format_idc (2 bits + 6 reserved)
    /// u8   bit_depth_luma_minus8 (3 bits + 5 reserved)
    /// u8   bit_depth_chroma_minus8 (3 bits + 5 reserved)
    /// u16  avgFrameRate
    /// u8   constantFrameRate(2) | numTemporalLayers(3) | temporalIdNested(1) | lengthSizeMinusOne(2)
    /// u8   numOfArrays
    /// for each array:
    ///   u8   array_completeness(1) | reserved(1) | NAL_unit_type(6)
    ///   u16  numNalus
    ///   for each nalu:
    ///     u16  nalUnitLength
    ///     u8[nalUnitLength] nalUnit
    /// ```
    pub fn parse(data: &[u8]) -> Result<Self, CodecError> {
        if data.len() < 23 {
            return Err(CodecError::InvalidConfig {
                message: "hvcC too short".to_string(),
            });
        }

        let version = data[0];
        if version != 1 {
            return Err(CodecError::InvalidConfig {
                message: format!("unsupported hvcC version {version}"),
            });
        }

        let general_profile_idc = data[1] & 0x1F;
        let general_level_idc = data[12];
        let nal_length_size = (data[21] & 0x03) + 1;
        let num_arrays = data[22] as usize;

        let mut vps = Vec::new();
        let mut sps = Vec::new();
        let mut pps = Vec::new();
        let mut pos = 23;

        for _ in 0..num_arrays {
            if pos >= data.len() {
                return Err(CodecError::InvalidConfig {
                    message: "hvcC truncated in array header".to_string(),
                });
            }

            let nal_unit_type = data[pos] & 0x3F;
            pos += 1;

            if pos + 2 > data.len() {
                return Err(CodecError::InvalidConfig {
                    message: "hvcC truncated in array count".to_string(),
                });
            }

            let num_nalus = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;

            for _ in 0..num_nalus {
                if pos + 2 > data.len() {
                    return Err(CodecError::InvalidConfig {
                        message: "hvcC truncated in NAL length".to_string(),
                    });
                }

                let nal_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;

                if pos + nal_len > data.len() {
                    return Err(CodecError::InvalidConfig {
                        message: "hvcC NAL data truncated".to_string(),
                    });
                }

                let nal_data = data[pos..pos + nal_len].to_vec();
                pos += nal_len;

                match nal_unit_type {
                    32 => vps.push(nal_data), // VPS
                    33 => sps.push(nal_data), // SPS
                    34 => pps.push(nal_data), // PPS
                    _ => {}                   // Skip other NAL types (SEI, etc.)
                }
            }
        }

        Ok(Self {
            nal_length_size,
            general_profile_idc,
            general_level_idc,
            vps,
            sps,
            pps,
        })
    }

    /// Produces Annex B format byte stream with VPS, SPS, and PPS.
    ///
    /// This is needed to initialize the libde265 decoder, which expects
    /// Annex B format (start code + NAL unit).
    pub fn to_annex_b(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for vps in &self.vps {
            out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
            out.extend_from_slice(vps);
        }
        for sps in &self.sps {
            out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
            out.extend_from_slice(sps);
        }
        for pps in &self.pps {
            out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
            out.extend_from_slice(pps);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_hvcc_parses_single_vps_sps_pps() {
        // GIVEN — a minimal hvcC with 1 VPS (2 bytes), 1 SPS (2 bytes), 1 PPS (2 bytes)
        let mut hvcc = vec![0u8; 23];
        hvcc[0] = 1; // version
        hvcc[1] = 0x01; // profile_idc = 1 (Main)
        hvcc[12] = 93; // level_idc = 93 (level 3.1)
        hvcc[21] = 0xFF; // length_size_minus_one = 3 (NAL length = 4), reserved bits set
        hvcc[22] = 3; // 3 arrays (VPS, SPS, PPS)

        // VPS array
        hvcc.push(0x20); // NAL type 32 (VPS)
        hvcc.extend_from_slice(&1u16.to_be_bytes()); // 1 NAL
        hvcc.extend_from_slice(&2u16.to_be_bytes()); // length 2
        hvcc.extend_from_slice(&[0x40, 0x01]); // VPS data

        // SPS array
        hvcc.push(0x21); // NAL type 33 (SPS)
        hvcc.extend_from_slice(&1u16.to_be_bytes());
        hvcc.extend_from_slice(&2u16.to_be_bytes());
        hvcc.extend_from_slice(&[0x42, 0x01]); // SPS data

        // PPS array
        hvcc.push(0x22); // NAL type 34 (PPS)
        hvcc.extend_from_slice(&1u16.to_be_bytes());
        hvcc.extend_from_slice(&2u16.to_be_bytes());
        hvcc.extend_from_slice(&[0x44, 0x01]); // PPS data

        // WHEN
        let config = HevcDecoderConfig::parse(&hvcc).unwrap();

        // THEN
        assert_eq!(config.nal_length_size, 4);
        assert_eq!(config.general_profile_idc, 1);
        assert_eq!(config.general_level_idc, 93);
        assert_eq!(config.vps.len(), 1);
        assert_eq!(config.vps[0], &[0x40, 0x01]);
        assert_eq!(config.sps.len(), 1);
        assert_eq!(config.sps[0], &[0x42, 0x01]);
        assert_eq!(config.pps.len(), 1);
        assert_eq!(config.pps[0], &[0x44, 0x01]);
    }

    #[test]
    fn test_that_hvcc_to_annex_b_adds_start_codes() {
        // GIVEN
        let config = HevcDecoderConfig {
            nal_length_size: 4,
            general_profile_idc: 1,
            general_level_idc: 93,
            vps: vec![vec![0x40, 0x01]],
            sps: vec![vec![0x42, 0x01]],
            pps: vec![vec![0x44, 0x01]],
        };

        // WHEN
        let annex_b = config.to_annex_b();

        // THEN
        assert_eq!(
            annex_b,
            vec![
                0x00, 0x00, 0x00, 0x01, 0x40, 0x01, // start code + VPS
                0x00, 0x00, 0x00, 0x01, 0x42, 0x01, // start code + SPS
                0x00, 0x00, 0x00, 0x01, 0x44, 0x01, // start code + PPS
            ]
        );
    }

    #[test]
    fn test_that_hvcc_rejects_truncated_data() {
        let result = HevcDecoderConfig::parse(&[1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_that_hvcc_skips_sei_nal_units() {
        // GIVEN — hvcC with VPS + SEI prefix (type 39) arrays
        let mut hvcc = vec![0u8; 23];
        hvcc[0] = 1;
        hvcc[1] = 0x01;
        hvcc[12] = 93;
        hvcc[21] = 0xFF;
        hvcc[22] = 2; // 2 arrays

        // VPS array
        hvcc.push(0x20); // NAL type 32
        hvcc.extend_from_slice(&1u16.to_be_bytes());
        hvcc.extend_from_slice(&2u16.to_be_bytes());
        hvcc.extend_from_slice(&[0x40, 0x01]);

        // SEI prefix array
        hvcc.push(39); // NAL type 39 (SEI prefix)
        hvcc.extend_from_slice(&1u16.to_be_bytes());
        hvcc.extend_from_slice(&3u16.to_be_bytes());
        hvcc.extend_from_slice(&[0xFF, 0xFE, 0xFD]);

        // WHEN
        let config = HevcDecoderConfig::parse(&hvcc).unwrap();

        // THEN — only VPS collected, SEI skipped
        assert_eq!(config.vps.len(), 1);
        assert_eq!(config.sps.len(), 0);
        assert_eq!(config.pps.len(), 0);
    }
}
