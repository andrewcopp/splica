//! Shared MP4 box building helpers used by both `Mp4Muxer` and `FragmentedMp4Muxer`.
//!
//! These functions produce raw byte vectors for ISO BMFF boxes that are
//! structurally identical between regular and fragmented MP4 files.

use splica_core::{
    ColorPrimaries, ColorRange, ColorSpace, MatrixCoefficients, MuxError, TransferCharacteristics,
};

use crate::boxes::hdlr::HandlerType;
use crate::boxes::stsd::CodecConfig;

/// Wraps `body` in a standard ISO BMFF box with the given four-character code.
pub(crate) fn make_box(fourcc: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = (8 + body.len()) as u32;
    let mut buf = Vec::with_capacity(size as usize);
    buf.extend_from_slice(&size.to_be_bytes());
    buf.extend_from_slice(fourcc);
    buf.extend_from_slice(body);
    buf
}

/// Wraps `body` in a full box (body already includes version+flags prefix).
pub(crate) fn make_full_box(fourcc: &[u8; 4], body: &[u8]) -> Vec<u8> {
    make_box(fourcc, body)
}

/// Builds an `mvhd` (Movie Header) box.
pub(crate) fn build_mvhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    body.extend_from_slice(&timescale.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&0x00010000u32.to_be_bytes()); // rate = 1.0
    body.extend_from_slice(&0x0100u16.to_be_bytes()); // volume = 1.0
    body.extend_from_slice(&[0u8; 10]); // reserved
    for &val in &[0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
        body.extend_from_slice(&val.to_be_bytes());
    }
    body.extend_from_slice(&[0u8; 24]); // pre_defined
    body.extend_from_slice(&0xFFFFFFFFu32.to_be_bytes()); // next_track_ID
    make_full_box(b"mvhd", &body)
}

/// Builds a `tkhd` (Track Header) box.
pub(crate) fn build_tkhd(track_id: u32, duration: u32, width: u32, height: u32) -> Vec<u8> {
    let mut body = Vec::new();
    // version=0, flags=3 (track_enabled | track_in_movie)
    body.extend_from_slice(&[0, 0, 0, 3]);
    body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    body.extend_from_slice(&track_id.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes()); // reserved
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&[0u8; 8]); // reserved
    body.extend_from_slice(&0u16.to_be_bytes()); // layer
    body.extend_from_slice(&0u16.to_be_bytes()); // alternate_group
    body.extend_from_slice(&0u16.to_be_bytes()); // volume (0 for video)
    body.extend_from_slice(&[0u8; 2]); // reserved
    for &val in &[0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
        body.extend_from_slice(&val.to_be_bytes());
    }
    // Width and height as 16.16 fixed-point
    body.extend_from_slice(&(width << 16).to_be_bytes());
    body.extend_from_slice(&(height << 16).to_be_bytes());
    make_full_box(b"tkhd", &body)
}

/// Builds an `mdhd` (Media Header) box.
pub(crate) fn build_mdhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    body.extend_from_slice(&timescale.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&0x55C4u16.to_be_bytes()); // language (undetermined)
    body.extend_from_slice(&0u16.to_be_bytes()); // pre_defined
    make_full_box(b"mdhd", &body)
}

/// Builds an `hdlr` (Handler Reference) box.
pub(crate) fn build_hdlr(handler_type: HandlerType) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    body.extend_from_slice(handler_type.as_bytes());
    body.extend_from_slice(&[0u8; 12]); // reserved
    body.push(0); // name (null-terminated empty string)
    make_full_box(b"hdlr", &body)
}

/// Builds a `vmhd` (Video Media Header) box.
pub(crate) fn build_vmhd() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 1]); // version=0, flags=1
    body.extend_from_slice(&0u16.to_be_bytes()); // graphicsmode
    body.extend_from_slice(&[0u8; 6]); // opcolor
    make_full_box(b"vmhd", &body)
}

/// Builds an `smhd` (Sound Media Header) box.
pub(crate) fn build_smhd() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&0u16.to_be_bytes()); // balance
    body.extend_from_slice(&0u16.to_be_bytes()); // reserved
    make_full_box(b"smhd", &body)
}

/// Builds an `nmhd` (Null Media Header) box for subtitle/text tracks.
pub(crate) fn build_nmhd() -> Vec<u8> {
    let body = [0u8; 4]; // version=0, flags=0
    make_full_box(b"nmhd", &body)
}

/// Builds a `dinf` (Data Information) box containing a self-contained `dref`.
pub(crate) fn build_dinf() -> Vec<u8> {
    let mut url_body = Vec::new();
    url_body.extend_from_slice(&[0, 0, 0, 1]); // version=0, flags=1 (self-contained)
    let url_box = make_full_box(b"url ", &url_body);

    let mut dref_body = Vec::new();
    dref_body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    dref_body.extend_from_slice(&1u32.to_be_bytes()); // entry_count
    dref_body.extend_from_slice(&url_box);
    let dref = make_full_box(b"dref", &dref_body);

    make_box(b"dinf", &dref)
}

/// Builds a visual sample entry box (avc1, hev1, av01, etc.).
pub(crate) fn build_visual_sample_entry(
    fourcc: &[u8; 4],
    width: u16,
    height: u16,
    config_fourcc: &[u8; 4],
    config_data: &[u8],
    color_space: Option<&ColorSpace>,
) -> Vec<u8> {
    let mut entry = Vec::new();
    entry.extend_from_slice(&[0u8; 6]); // reserved
    entry.extend_from_slice(&1u16.to_be_bytes()); // data_ref_index
    entry.extend_from_slice(&[0u8; 16]); // pre_defined + reserved
    entry.extend_from_slice(&width.to_be_bytes());
    entry.extend_from_slice(&height.to_be_bytes());
    entry.extend_from_slice(&0x00480000u32.to_be_bytes()); // horiz_res (72 dpi)
    entry.extend_from_slice(&0x00480000u32.to_be_bytes()); // vert_res (72 dpi)
    entry.extend_from_slice(&0u32.to_be_bytes()); // reserved
    entry.extend_from_slice(&1u16.to_be_bytes()); // frame_count
    entry.extend_from_slice(&[0u8; 32]); // compressor_name
    entry.extend_from_slice(&0x0018u16.to_be_bytes()); // depth = 24
    entry.extend_from_slice(&(-1i16).to_be_bytes()); // pre_defined

    if !config_data.is_empty() {
        let config_box = make_box(config_fourcc, config_data);
        entry.extend_from_slice(&config_box);
    }

    if let Some(cs) = color_space {
        let colr = build_colr_nclx(cs);
        entry.extend_from_slice(&colr);
    }

    make_box(fourcc, &entry)
}

/// Builds a colr box with nclx (ISO 23001-8) color type.
fn build_colr_nclx(cs: &ColorSpace) -> Vec<u8> {
    let primaries: u16 = match cs.primaries {
        ColorPrimaries::Bt709 => 1,
        ColorPrimaries::Bt2020 => 9,
        ColorPrimaries::Smpte432 => 12,
    };
    let transfer: u16 = match cs.transfer {
        TransferCharacteristics::Bt709 => 1,
        TransferCharacteristics::Smpte2084 => 16,
        TransferCharacteristics::HybridLogGamma => 18,
    };
    let matrix: u16 = match cs.matrix {
        MatrixCoefficients::Identity => 0,
        MatrixCoefficients::Bt709 => 1,
        MatrixCoefficients::Bt2020NonConstant => 9,
        MatrixCoefficients::Bt2020Constant => 10,
    };
    let full_range: u8 = match cs.range {
        ColorRange::Full => 0x80,
        ColorRange::Limited => 0x00,
    };

    let mut body = Vec::with_capacity(11);
    body.extend_from_slice(b"nclx");
    body.extend_from_slice(&primaries.to_be_bytes());
    body.extend_from_slice(&transfer.to_be_bytes());
    body.extend_from_slice(&matrix.to_be_bytes());
    body.push(full_range);

    make_box(b"colr", &body)
}

/// Builds an audio sample entry box (mp4a).
pub(crate) fn build_audio_sample_entry(
    sample_rate: u32,
    channel_count: u16,
    esds: &[u8],
) -> Vec<u8> {
    let mut entry = Vec::new();
    entry.extend_from_slice(&[0u8; 6]); // reserved
    entry.extend_from_slice(&1u16.to_be_bytes()); // data_ref_index
    entry.extend_from_slice(&[0u8; 8]); // reserved
    entry.extend_from_slice(&channel_count.to_be_bytes());
    entry.extend_from_slice(&16u16.to_be_bytes()); // sample_size = 16
    entry.extend_from_slice(&0u16.to_be_bytes()); // pre_defined
    entry.extend_from_slice(&0u16.to_be_bytes()); // reserved
    entry.extend_from_slice(&(sample_rate << 16).to_be_bytes()); // sample_rate (16.16)

    if !esds.is_empty() {
        let esds_box = make_full_box(b"esds", esds);
        entry.extend_from_slice(&esds_box);
    }

    make_box(b"mp4a", &entry)
}

/// Builds an `stsd` (Sample Description) box from a codec configuration.
pub(crate) fn build_stsd(config: &CodecConfig) -> Result<Vec<u8>, MuxError> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version + flags
    body.extend_from_slice(&1u32.to_be_bytes()); // entry count

    match config {
        CodecConfig::Avc1 {
            width,
            height,
            avcc,
            color_space,
        } => {
            let entry = build_visual_sample_entry(
                b"avc1",
                *width,
                *height,
                b"avcC",
                avcc,
                color_space.as_ref(),
            );
            body.extend_from_slice(&entry);
        }
        CodecConfig::Hev1 {
            width,
            height,
            hvcc,
            color_space,
        } => {
            let entry = build_visual_sample_entry(
                b"hev1",
                *width,
                *height,
                b"hvcC",
                hvcc,
                color_space.as_ref(),
            );
            body.extend_from_slice(&entry);
        }
        CodecConfig::Av1 {
            width,
            height,
            av1c,
            color_space,
        } => {
            let entry = build_visual_sample_entry(
                b"av01",
                *width,
                *height,
                b"av1C",
                av1c,
                color_space.as_ref(),
            );
            body.extend_from_slice(&entry);
        }
        CodecConfig::Mp4a {
            sample_rate,
            channel_count,
            esds,
        } => {
            let entry = build_audio_sample_entry(*sample_rate, *channel_count, esds);
            body.extend_from_slice(&entry);
        }
        CodecConfig::Unknown(name) => {
            return Err(MuxError::IncompatibleCodec {
                codec: name.clone(),
                container: "mp4".to_string(),
            });
        }
    }

    Ok(make_full_box(b"stsd", &body))
}

/// Converts an `io::Error` into a `MuxError`.
pub(crate) fn io_err(e: std::io::Error) -> MuxError {
    MuxError::Io(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boxes::stsd::parse_stsd;

    #[test]
    fn test_that_colr_box_roundtrips_through_build_and_parse() {
        // GIVEN — an Avc1 config with BT.709 color space
        let color_space = ColorSpace {
            primaries: ColorPrimaries::Bt709,
            transfer: TransferCharacteristics::Bt709,
            matrix: MatrixCoefficients::Bt709,
            range: ColorRange::Limited,
        };
        let config = CodecConfig::Avc1 {
            width: 1920,
            height: 1080,
            avcc: bytes::Bytes::from_static(&[1, 0x42, 0xC0, 0x1E, 0xFF]),
            color_space: Some(color_space),
        };

        // WHEN — build stsd and parse it back
        let stsd_bytes = build_stsd(&config).unwrap();
        let parsed = parse_stsd(&stsd_bytes[8..], 0).unwrap();

        // THEN — color space should survive the roundtrip
        match parsed {
            CodecConfig::Avc1 {
                width,
                height,
                color_space: cs,
                ..
            } => {
                assert_eq!(width, 1920);
                assert_eq!(height, 1080);
                let cs = cs.expect("color_space should be present after roundtrip");
                assert_eq!(cs.primaries, ColorPrimaries::Bt709);
                assert_eq!(cs.transfer, TransferCharacteristics::Bt709);
                assert_eq!(cs.matrix, MatrixCoefficients::Bt709);
                assert_eq!(cs.range, ColorRange::Limited);
            }
            other => panic!("expected Avc1, got {other:?}"),
        }
    }

    #[test]
    fn test_that_bt2020_pq_color_space_roundtrips() {
        // GIVEN — BT.2020 PQ (HDR10) color space
        let color_space = ColorSpace {
            primaries: ColorPrimaries::Bt2020,
            transfer: TransferCharacteristics::Smpte2084,
            matrix: MatrixCoefficients::Bt2020NonConstant,
            range: ColorRange::Full,
        };
        let config = CodecConfig::Hev1 {
            width: 3840,
            height: 2160,
            hvcc: bytes::Bytes::from_static(&[1, 0]),
            color_space: Some(color_space),
        };

        // WHEN
        let stsd_bytes = build_stsd(&config).unwrap();
        let parsed = parse_stsd(&stsd_bytes[8..], 0).unwrap();

        // THEN
        match parsed {
            CodecConfig::Hev1 {
                color_space: cs, ..
            } => {
                let cs = cs.expect("color_space should be present");
                assert_eq!(cs.primaries, ColorPrimaries::Bt2020);
                assert_eq!(cs.transfer, TransferCharacteristics::Smpte2084);
                assert_eq!(cs.matrix, MatrixCoefficients::Bt2020NonConstant);
                assert_eq!(cs.range, ColorRange::Full);
            }
            other => panic!("expected Hev1, got {other:?}"),
        }
    }

    #[test]
    fn test_that_stsd_without_color_space_roundtrips_as_none() {
        // GIVEN — Avc1 config with no color space
        let config = CodecConfig::Avc1 {
            width: 640,
            height: 480,
            avcc: bytes::Bytes::from_static(&[1, 0x42, 0xC0, 0x1E, 0xFF]),
            color_space: None,
        };

        // WHEN
        let stsd_bytes = build_stsd(&config).unwrap();
        let parsed = parse_stsd(&stsd_bytes[8..], 0).unwrap();

        // THEN
        match parsed {
            CodecConfig::Avc1 { color_space, .. } => {
                assert!(
                    color_space.is_none(),
                    "color_space should be None when not set"
                );
            }
            other => panic!("expected Avc1, got {other:?}"),
        }
    }
}
