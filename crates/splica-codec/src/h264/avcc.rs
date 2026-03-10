//! Parser for the AVC Decoder Configuration Record (avcC box body).
//!
//! Extracts SPS/PPS NAL units and the NAL length size used in MP4 sample data.

use crate::error::CodecError;

/// Parsed contents of an avcC (AVC Decoder Configuration Record).
#[derive(Debug, Clone)]
pub struct AvcDecoderConfig {
    /// Number of bytes used for NAL unit length fields in sample data (typically 4).
    pub nal_length_size: u8,
    /// AVC profile indication (e.g., 0x42 = Baseline, 0x4D = Main, 0x64 = High).
    pub profile_idc: u8,
    /// AVC level indication (e.g., 0x1E = 3.0, 0x28 = 4.0).
    pub level_idc: u8,
    /// Sequence Parameter Set NAL units.
    pub sps: Vec<Vec<u8>>,
    /// Picture Parameter Set NAL units.
    pub pps: Vec<Vec<u8>>,
}

impl AvcDecoderConfig {
    /// Parses an avcC box body into its constituent parts.
    ///
    /// Layout (ISO/IEC 14496-15):
    /// ```text
    /// u8  configuration_version (must be 1)
    /// u8  avc_profile_indication
    /// u8  profile_compatibility
    /// u8  avc_level_indication
    /// u8  length_size_minus_one (lower 2 bits) | reserved (upper 6 bits = 0b111111)
    /// u8  num_sps (lower 5 bits) | reserved (upper 3 bits = 0b111)
    /// for each SPS:
    ///   u16 sps_length
    ///   u8[sps_length] sps_nal_unit
    /// u8  num_pps
    /// for each PPS:
    ///   u16 pps_length
    ///   u8[pps_length] pps_nal_unit
    /// ```
    pub fn parse(data: &[u8]) -> Result<Self, CodecError> {
        if data.len() < 7 {
            return Err(CodecError::InvalidConfig {
                message: "avcC too short".to_string(),
            });
        }

        let version = data[0];
        if version != 1 {
            return Err(CodecError::InvalidConfig {
                message: format!("unsupported avcC version {version}"),
            });
        }

        let profile_idc = data[1];
        let level_idc = data[3];
        let nal_length_size = (data[4] & 0x03) + 1;
        let num_sps = (data[5] & 0x1F) as usize;

        let mut pos = 6;
        let mut sps = Vec::with_capacity(num_sps);

        for _ in 0..num_sps {
            if pos + 2 > data.len() {
                return Err(CodecError::InvalidConfig {
                    message: "avcC truncated in SPS".to_string(),
                });
            }
            let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + len > data.len() {
                return Err(CodecError::InvalidConfig {
                    message: "avcC SPS data truncated".to_string(),
                });
            }
            sps.push(data[pos..pos + len].to_vec());
            pos += len;
        }

        if pos >= data.len() {
            return Err(CodecError::InvalidConfig {
                message: "avcC truncated before PPS count".to_string(),
            });
        }

        let num_pps = data[pos] as usize;
        pos += 1;
        let mut pps = Vec::with_capacity(num_pps);

        for _ in 0..num_pps {
            if pos + 2 > data.len() {
                return Err(CodecError::InvalidConfig {
                    message: "avcC truncated in PPS".to_string(),
                });
            }
            let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + len > data.len() {
                return Err(CodecError::InvalidConfig {
                    message: "avcC PPS data truncated".to_string(),
                });
            }
            pps.push(data[pos..pos + len].to_vec());
            pos += len;
        }

        Ok(Self {
            nal_length_size,
            profile_idc,
            level_idc,
            sps,
            pps,
        })
    }

    /// Produces Annex B format byte stream with SPS and PPS preceded by start codes.
    ///
    /// This is needed to initialize the openh264 decoder, which expects
    /// Annex B format (start code + NAL unit).
    pub fn to_annex_b(&self) -> Vec<u8> {
        let mut out = Vec::new();
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

/// Converts MP4 sample data (length-prefixed NAL units) to Annex B format
/// (start code prefixed NAL units).
///
/// MP4 stores H.264 with each NAL unit preceded by its length (typically 4 bytes).
/// openh264 expects Annex B format with `00 00 00 01` start codes instead.
pub fn mp4_to_annex_b(data: &[u8], nal_length_size: u8) -> Result<Vec<u8>, CodecError> {
    let nal_len_bytes = nal_length_size as usize;
    let mut out = Vec::with_capacity(data.len());
    let mut pos = 0;

    while pos + nal_len_bytes <= data.len() {
        let nal_len = match nal_len_bytes {
            1 => data[pos] as usize,
            2 => u16::from_be_bytes([data[pos], data[pos + 1]]) as usize,
            4 => u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                as usize,
            _ => {
                return Err(CodecError::InvalidConfig {
                    message: format!("unsupported NAL length size {nal_len_bytes}"),
                })
            }
        };
        pos += nal_len_bytes;

        if pos + nal_len > data.len() {
            return Err(CodecError::InvalidBitstream {
                message: format!(
                    "NAL unit length {nal_len} exceeds remaining data {} at offset {}",
                    data.len() - pos,
                    pos - nal_len_bytes
                ),
            });
        }

        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        out.extend_from_slice(&data[pos..pos + nal_len]);
        pos += nal_len;
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_avcc_parses_single_sps_pps() {
        // GIVEN — a minimal avcC with 1 SPS (2 bytes) and 1 PPS (2 bytes)
        let avcc = vec![
            1,    // version
            0x42, // profile (Baseline)
            0xC0, // compatibility
            0x1E, // level 3.0
            0xFF, // length_size_minus_one = 3 (NAL length = 4 bytes), reserved bits set
            0xE1, // num_sps = 1, reserved bits set
            0x00, 0x02, // sps_length = 2
            0x67, 0x42, // SPS NAL data (fake)
            0x01, // num_pps = 1
            0x00, 0x02, // pps_length = 2
            0x68, 0xCE, // PPS NAL data (fake)
        ];

        // WHEN
        let config = AvcDecoderConfig::parse(&avcc).unwrap();

        // THEN
        assert_eq!(config.nal_length_size, 4);
        assert_eq!(config.profile_idc, 0x42);
        assert_eq!(config.level_idc, 0x1E);
        assert_eq!(config.sps.len(), 1);
        assert_eq!(config.sps[0], &[0x67, 0x42]);
        assert_eq!(config.pps.len(), 1);
        assert_eq!(config.pps[0], &[0x68, 0xCE]);
    }

    #[test]
    fn test_that_avcc_to_annex_b_adds_start_codes() {
        // GIVEN
        let config = AvcDecoderConfig {
            nal_length_size: 4,
            profile_idc: 0x42,
            level_idc: 0x1E,
            sps: vec![vec![0x67, 0x42]],
            pps: vec![vec![0x68, 0xCE]],
        };

        // WHEN
        let annex_b = config.to_annex_b();

        // THEN
        assert_eq!(
            annex_b,
            vec![
                0x00, 0x00, 0x00, 0x01, 0x67, 0x42, // start code + SPS
                0x00, 0x00, 0x00, 0x01, 0x68, 0xCE, // start code + PPS
            ]
        );
    }

    #[test]
    fn test_that_mp4_to_annex_b_converts_length_prefixed_nals() {
        // GIVEN — two NAL units with 4-byte length prefix
        let data = vec![
            0x00, 0x00, 0x00, 0x03, // length = 3
            0x65, 0x88, 0x80, // NAL data (IDR slice)
            0x00, 0x00, 0x00, 0x02, // length = 2
            0x41, 0x9A, // NAL data (non-IDR slice)
        ];

        // WHEN
        let annex_b = mp4_to_annex_b(&data, 4).unwrap();

        // THEN
        assert_eq!(
            annex_b,
            vec![
                0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x80, // start code + IDR
                0x00, 0x00, 0x00, 0x01, 0x41, 0x9A, // start code + non-IDR
            ]
        );
    }

    #[test]
    fn test_that_avcc_rejects_truncated_data() {
        // GIVEN — too short
        let result = AvcDecoderConfig::parse(&[1, 2, 3]);

        // THEN
        assert!(result.is_err());
    }
}
