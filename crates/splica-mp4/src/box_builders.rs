//! Shared MP4 box building helpers used by both `Mp4Muxer` and `FragmentedMp4Muxer`.
//!
//! These functions produce raw byte vectors for ISO BMFF boxes that are
//! structurally identical between regular and fragmented MP4 files.

use splica_core::MuxError;

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
pub(crate) fn build_hdlr(handler_type: &[u8; 4]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    body.extend_from_slice(handler_type);
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

    make_box(fourcc, &entry)
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
        } => {
            let entry = build_visual_sample_entry(b"avc1", *width, *height, b"avcC", avcc);
            body.extend_from_slice(&entry);
        }
        CodecConfig::Hev1 {
            width,
            height,
            hvcc,
        } => {
            let entry = build_visual_sample_entry(b"hev1", *width, *height, b"hvcC", hvcc);
            body.extend_from_slice(&entry);
        }
        CodecConfig::Av1 {
            width,
            height,
            av1c,
        } => {
            let entry = build_visual_sample_entry(b"av01", *width, *height, b"av1C", av1c);
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
