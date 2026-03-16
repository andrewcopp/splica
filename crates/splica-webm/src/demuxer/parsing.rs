//! EBML parsing helpers for the WebM demuxer.
//!
//! Standalone functions that parse EBML elements (Info, Tracks, SimpleBlock, etc.)
//! into domain types. These are pure parsers with no dependency on `WebmDemuxer` state.

use std::io::{Read, Seek, SeekFrom};

use splica_core::{
    AudioCodec, AudioTrackInfo, ChannelLayout, Codec, Timestamp, TrackIndex, TrackInfo, TrackKind,
    VideoCodec, VideoTrackInfo,
};

use crate::ebml;
use crate::elements;
use crate::error::WebmError;

/// Internal track metadata parsed from the Tracks element.
pub(crate) struct WebmTrack {
    /// Matroska track number (1-based, from TrackNumber element).
    pub(crate) track_number: u64,
    /// Track type: video (1) or audio (2).
    track_type: u64,
    /// Codec identifier string (e.g., "V_VP9", "A_OPUS").
    codec_id: String,
    /// Optional codec private data (used for codec initialization).
    pub(crate) codec_private: Option<Vec<u8>>,
    /// Video dimensions (if video track).
    width: Option<u32>,
    height: Option<u32>,
    /// Audio parameters (if audio track).
    sample_rate: Option<f64>,
    channels: Option<u64>,
}

/// A buffered packet ready to be yielded.
pub(crate) struct BufferedPacket {
    pub(crate) track_number: u64,
    pub(crate) pts_ns: i64,
    pub(crate) is_keyframe: bool,
    pub(crate) data: Vec<u8>,
}

/// Segment-level info: timestamp scale and optional duration.
pub(crate) struct SegmentInfo {
    pub(crate) timestamp_scale: u64,
    /// Segment duration in nanoseconds, if present.
    pub(crate) duration_ns: Option<f64>,
}

/// Reads an EBML element header directly from a reader without buffering the
/// entire file. Seeks the reader to just after the header (start of body).
/// Returns the parsed header and the number of bytes consumed.
pub(crate) fn read_element_header_from_reader<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
) -> Result<(ebml::ElementHeader, usize), WebmError> {
    // Element headers are at most 12 bytes (4-byte ID + 8-byte size)
    let mut buf = [0u8; 12];
    // Read enough for at least a minimal header (2 bytes)
    let mut filled = 0;
    while filled < 2 {
        let n = reader.read(&mut buf[filled..])?;
        if n == 0 {
            return Err(WebmError::UnexpectedEof { offset });
        }
        filled += n;
    }
    // Try parsing; if we need more bytes, read more
    loop {
        match ebml::parse_element_header(&buf[..filled], offset) {
            Ok(header) => {
                // Seek reader to start of element body
                reader.seek(SeekFrom::Start(offset + header.header_size as u64))?;
                return Ok((header, header.header_size));
            }
            Err(_) if filled < buf.len() => {
                let n = reader.read(&mut buf[filled..])?;
                if n == 0 {
                    return Err(WebmError::UnexpectedEof { offset });
                }
                filled += n;
            }
            Err(e) => return Err(e),
        }
    }
}

pub(crate) fn parse_ebml_doc_type(data: &[u8], base_offset: u64) -> Result<String, WebmError> {
    let mut pos: usize = 0;
    while pos < data.len() {
        let header = ebml::parse_element_header(&data[pos..], base_offset + pos as u64)?;
        let body_start = pos + header.header_size;
        let size = header.data_size.unwrap_or(0) as usize;

        if body_start + size > data.len() {
            break;
        }

        if header.id == elements::EBML_DOC_TYPE {
            return ebml::read_string(&data[body_start..body_start + size], base_offset);
        }

        pos = body_start + size;
    }

    Err(WebmError::MissingElement {
        name: "EBMLDocType",
    })
}

pub(crate) fn parse_info(data: &[u8], base_offset: u64) -> Result<SegmentInfo, WebmError> {
    let mut timestamp_scale: u64 = 1_000_000;
    let mut duration_ticks: Option<f64> = None;
    let mut pos: usize = 0;

    while pos < data.len() {
        let header = match ebml::parse_element_header(&data[pos..], base_offset + pos as u64) {
            Ok(h) => h,
            Err(_) => break,
        };
        let body_start = pos + header.header_size;
        let size = header.data_size.unwrap_or(0) as usize;

        if body_start + size > data.len() {
            break;
        }

        match header.id {
            elements::TIMESTAMP_SCALE => {
                timestamp_scale =
                    ebml::read_uint(&data[body_start..body_start + size], base_offset)?;
            }
            elements::DURATION => {
                duration_ticks = Some(ebml::read_float(
                    &data[body_start..body_start + size],
                    base_offset,
                )?);
            }
            _ => {}
        }

        pos = body_start + size;
    }

    let duration_ns = duration_ticks.map(|ticks| ticks * timestamp_scale as f64);

    Ok(SegmentInfo {
        timestamp_scale,
        duration_ns,
    })
}

pub(crate) fn parse_tracks(data: &[u8], base_offset: u64) -> Result<Vec<WebmTrack>, WebmError> {
    let mut tracks = Vec::new();
    let mut pos: usize = 0;

    while pos < data.len() {
        let header = match ebml::parse_element_header(&data[pos..], base_offset + pos as u64) {
            Ok(h) => h,
            Err(_) => break,
        };
        let body_start = pos + header.header_size;
        let size = header.data_size.unwrap_or(0) as usize;

        if body_start + size > data.len() {
            break;
        }

        if header.id == elements::TRACK_ENTRY {
            let track = parse_track_entry(
                &data[body_start..body_start + size],
                base_offset + body_start as u64,
            )?;
            tracks.push(track);
        }

        pos = body_start + size;
    }

    Ok(tracks)
}

fn parse_track_entry(data: &[u8], base_offset: u64) -> Result<WebmTrack, WebmError> {
    let mut track_number: u64 = 0;
    let mut track_type: u64 = 0;
    let mut codec_id = String::new();
    let mut codec_private: Option<Vec<u8>> = None;
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut sample_rate: Option<f64> = None;
    let mut channels: Option<u64> = None;

    let mut pos: usize = 0;
    while pos < data.len() {
        let header = match ebml::parse_element_header(&data[pos..], base_offset + pos as u64) {
            Ok(h) => h,
            Err(_) => break,
        };
        let body_start = pos + header.header_size;
        let size = header.data_size.unwrap_or(0) as usize;

        if body_start + size > data.len() {
            break;
        }

        let body = &data[body_start..body_start + size];

        match header.id {
            elements::TRACK_NUMBER => {
                track_number = ebml::read_uint(body, base_offset)?;
            }
            elements::TRACK_TYPE => {
                track_type = ebml::read_uint(body, base_offset)?;
            }
            elements::CODEC_ID => {
                codec_id = ebml::read_string(body, base_offset)?;
            }
            elements::CODEC_PRIVATE => {
                codec_private = Some(body.to_vec());
            }
            elements::VIDEO => {
                let (w, h) = parse_video_settings(body, base_offset + body_start as u64)?;
                width = Some(w);
                height = Some(h);
            }
            elements::AUDIO => {
                let (sr, ch) = parse_audio_settings(body, base_offset + body_start as u64)?;
                sample_rate = Some(sr);
                channels = Some(ch);
            }
            _ => {}
        }

        pos = body_start + size;
    }

    Ok(WebmTrack {
        track_number,
        track_type,
        codec_id,
        codec_private,
        width,
        height,
        sample_rate,
        channels,
    })
}

fn parse_video_settings(data: &[u8], base_offset: u64) -> Result<(u32, u32), WebmError> {
    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut pos: usize = 0;

    while pos < data.len() {
        let header = match ebml::parse_element_header(&data[pos..], base_offset + pos as u64) {
            Ok(h) => h,
            Err(_) => break,
        };
        let body_start = pos + header.header_size;
        let size = header.data_size.unwrap_or(0) as usize;

        if body_start + size > data.len() {
            break;
        }

        let body = &data[body_start..body_start + size];

        match header.id {
            elements::PIXEL_WIDTH => {
                width = ebml::read_uint(body, base_offset)? as u32;
            }
            elements::PIXEL_HEIGHT => {
                height = ebml::read_uint(body, base_offset)? as u32;
            }
            _ => {}
        }

        pos = body_start + size;
    }

    Ok((width, height))
}

fn parse_audio_settings(data: &[u8], base_offset: u64) -> Result<(f64, u64), WebmError> {
    let mut sample_rate: f64 = 8000.0;
    let mut channels: u64 = 1;
    let mut pos: usize = 0;

    while pos < data.len() {
        let header = match ebml::parse_element_header(&data[pos..], base_offset + pos as u64) {
            Ok(h) => h,
            Err(_) => break,
        };
        let body_start = pos + header.header_size;
        let size = header.data_size.unwrap_or(0) as usize;

        if body_start + size > data.len() {
            break;
        }

        let body = &data[body_start..body_start + size];

        match header.id {
            elements::SAMPLING_FREQUENCY => {
                sample_rate = ebml::read_float(body, base_offset)?;
            }
            elements::CHANNELS => {
                channels = ebml::read_uint(body, base_offset)?;
            }
            _ => {}
        }

        pos = body_start + size;
    }

    Ok((sample_rate, channels))
}

/// Parses a SimpleBlock element body into a buffered packet.
pub(crate) fn parse_simple_block(
    data: &[u8],
    cluster_timestamp: u64,
    timestamp_scale: u64,
    offset: u64,
) -> Result<Option<BufferedPacket>, WebmError> {
    if data.is_empty() {
        return Err(WebmError::UnexpectedEof { offset });
    }

    // Track number is a vint
    let (track_number_raw, tn_len) = ebml::read_vint(data, offset)?;
    // Strip the vint marker bit to get the track number
    let tn_width = data[0].leading_zeros() as usize + 1;
    let mask = 1u64 << (7 * tn_width);
    let track_number = track_number_raw & (mask - 1);

    let remaining = &data[tn_len..];
    if remaining.len() < 3 {
        return Err(WebmError::UnexpectedEof { offset });
    }

    // 2-byte signed timestamp relative to cluster
    let relative_timestamp = i16::from_be_bytes([remaining[0], remaining[1]]) as i64;

    // Flags byte
    let flags = remaining[2];
    let is_keyframe = (flags & 0x80) != 0;

    // Frame data starts after track_number + 2 bytes timestamp + 1 byte flags
    let frame_data = &remaining[3..];

    // Compute absolute timestamp in nanoseconds
    let abs_timestamp_ticks = cluster_timestamp as i64 + relative_timestamp;
    let pts_ns = abs_timestamp_ticks * timestamp_scale as i64;

    Ok(Some(BufferedPacket {
        track_number,
        pts_ns,
        is_keyframe,
        data: frame_data.to_vec(),
    }))
}

pub(crate) fn build_track_info(
    index: usize,
    track: &WebmTrack,
    segment_duration_ns: Option<f64>,
) -> Result<TrackInfo, WebmError> {
    let (kind, codec) = match track.track_type {
        elements::TRACK_TYPE_VIDEO => {
            let video_codec = match track.codec_id.as_str() {
                elements::CODEC_ID_VP8 => VideoCodec::Other("VP8".to_string()),
                elements::CODEC_ID_VP9 => VideoCodec::Other("VP9".to_string()),
                elements::CODEC_ID_AV1 => VideoCodec::Av1,
                elements::CODEC_ID_H264 => VideoCodec::H264,
                elements::CODEC_ID_H265 => VideoCodec::H265,
                other => {
                    return Err(WebmError::UnsupportedCodec {
                        codec_id: other.to_string(),
                    })
                }
            };
            (TrackKind::Video, Codec::Video(video_codec))
        }
        elements::TRACK_TYPE_AUDIO => {
            let audio_codec = match track.codec_id.as_str() {
                elements::CODEC_ID_VORBIS => AudioCodec::Other("Vorbis".to_string()),
                elements::CODEC_ID_OPUS => AudioCodec::Opus,
                elements::CODEC_ID_AAC => AudioCodec::Aac,
                other => {
                    return Err(WebmError::UnsupportedCodec {
                        codec_id: other.to_string(),
                    })
                }
            };
            (TrackKind::Audio, Codec::Audio(audio_codec))
        }
        _ => {
            return Err(WebmError::UnsupportedCodec {
                codec_id: format!("track type {}", track.track_type),
            })
        }
    };

    let video = if kind == TrackKind::Video {
        Some(VideoTrackInfo {
            width: track.width.unwrap_or(0),
            height: track.height.unwrap_or(0),
            pixel_format: None,
            color_space: None,
            frame_rate: None,
        })
    } else {
        None
    };

    let audio = if kind == TrackKind::Audio {
        let sr = track.sample_rate.unwrap_or(48000.0) as u32;
        let ch = track.channels.unwrap_or(2);
        let channel_layout = match ch {
            1 => Some(ChannelLayout::Mono),
            2 => Some(ChannelLayout::Stereo),
            6 => Some(ChannelLayout::Surround5_1),
            8 => Some(ChannelLayout::Surround7_1),
            _ => None,
        };
        Some(AudioTrackInfo {
            sample_rate: sr,
            channel_layout,
            sample_format: None,
        })
    } else {
        None
    };

    // WebM/MKV stores duration at the segment level, not per-track.
    // Convert nanoseconds to a Timestamp with nanosecond timebase.
    let duration = segment_duration_ns.and_then(|ns| {
        let ticks = ns.round() as i64;
        if ticks > 0 {
            Timestamp::new(ticks, 1_000_000_000)
        } else {
            None
        }
    });

    Ok(TrackInfo {
        index: TrackIndex(index as u32),
        kind,
        codec,
        duration,
        video,
        audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use splica_core::{AudioCodec, ChannelLayout, Codec, TrackKind, VideoCodec};

    // -----------------------------------------------------------------------
    // EBML encoding helpers (mirrors crate::ebml encoding functions)
    // -----------------------------------------------------------------------

    fn encode_element_id(id: u32) -> Vec<u8> {
        if id <= 0xFF {
            vec![id as u8]
        } else if id <= 0xFFFF {
            vec![(id >> 8) as u8, id as u8]
        } else if id <= 0xFF_FFFF {
            vec![(id >> 16) as u8, (id >> 8) as u8, id as u8]
        } else {
            vec![
                (id >> 24) as u8,
                (id >> 16) as u8,
                (id >> 8) as u8,
                id as u8,
            ]
        }
    }

    fn encode_data_size(size: u64) -> Vec<u8> {
        if size < 0x7F {
            vec![(size as u8) | 0x80]
        } else if size < 0x3FFF {
            let val = size | 0x4000;
            vec![(val >> 8) as u8, val as u8]
        } else {
            let val = size | 0x10_000000;
            vec![
                (val >> 24) as u8,
                (val >> 16) as u8,
                (val >> 8) as u8,
                val as u8,
            ]
        }
    }

    fn element(id: u32, body: &[u8]) -> Vec<u8> {
        let mut out = encode_element_id(id);
        out.extend_from_slice(&encode_data_size(body.len() as u64));
        out.extend_from_slice(body);
        out
    }

    fn uint_element(id: u32, value: u64) -> Vec<u8> {
        let bytes = if value == 0 {
            vec![0]
        } else if value <= 0xFF {
            vec![value as u8]
        } else if value <= 0xFFFF {
            vec![(value >> 8) as u8, value as u8]
        } else if value <= 0xFF_FFFF {
            vec![(value >> 16) as u8, (value >> 8) as u8, value as u8]
        } else {
            value.to_be_bytes().to_vec()
        };
        element(id, &bytes)
    }

    fn string_element(id: u32, s: &str) -> Vec<u8> {
        element(id, s.as_bytes())
    }

    fn float_element(id: u32, value: f64) -> Vec<u8> {
        element(id, &value.to_be_bytes())
    }

    // -----------------------------------------------------------------------
    // parse_ebml_doc_type
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_parse_ebml_doc_type_returns_webm_string() {
        // GIVEN — an EBML header body containing a DocType element set to "webm"
        let data = string_element(elements::EBML_DOC_TYPE, "webm");

        // WHEN — we parse the doc type
        let doc_type = parse_ebml_doc_type(&data, 0).unwrap();

        // THEN — it should be "webm"
        assert_eq!(doc_type, "webm");
    }

    #[test]
    fn test_that_parse_ebml_doc_type_returns_matroska_string() {
        // GIVEN — an EBML header body containing DocType "matroska"
        let data = string_element(elements::EBML_DOC_TYPE, "matroska");

        // WHEN — we parse the doc type
        let doc_type = parse_ebml_doc_type(&data, 0).unwrap();

        // THEN — it should be "matroska"
        assert_eq!(doc_type, "matroska");
    }

    #[test]
    fn test_that_parse_ebml_doc_type_errors_when_missing() {
        // GIVEN — an EBML header body with no DocType element
        let data = uint_element(elements::EBML_VERSION, 1);

        // WHEN — we parse the doc type
        let result = parse_ebml_doc_type(&data, 0);

        // THEN — it should fail with MissingElement
        assert!(result.is_err());
    }

    #[test]
    fn test_that_parse_ebml_doc_type_skips_unrelated_elements() {
        // GIVEN — an EBML header body with a version element before the DocType
        let data = [
            uint_element(elements::EBML_VERSION, 1),
            string_element(elements::EBML_DOC_TYPE, "webm"),
        ]
        .concat();

        // WHEN — we parse the doc type
        let doc_type = parse_ebml_doc_type(&data, 0).unwrap();

        // THEN — it should find "webm" after skipping unrelated elements
        assert_eq!(doc_type, "webm");
    }

    // -----------------------------------------------------------------------
    // parse_info
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_parse_info_returns_default_timestamp_scale() {
        // GIVEN — an empty Info element (no sub-elements)
        let data: &[u8] = &[];

        // WHEN — we parse the info
        let info = parse_info(data, 0).unwrap();

        // THEN — timestamp_scale defaults to 1_000_000
        assert_eq!(info.timestamp_scale, 1_000_000);
    }

    #[test]
    fn test_that_parse_info_reads_custom_timestamp_scale() {
        // GIVEN — an Info body with TimestampScale set to 500_000
        let data = uint_element(elements::TIMESTAMP_SCALE, 500_000);

        // WHEN — we parse the info
        let info = parse_info(&data, 0).unwrap();

        // THEN — timestamp_scale should be 500_000
        assert_eq!(info.timestamp_scale, 500_000);
    }

    #[test]
    fn test_that_parse_info_returns_none_duration_when_absent() {
        // GIVEN — an Info body with only TimestampScale
        let data = uint_element(elements::TIMESTAMP_SCALE, 1_000_000);

        // WHEN — we parse the info
        let info = parse_info(&data, 0).unwrap();

        // THEN — duration_ns should be None
        assert!(info.duration_ns.is_none());
    }

    #[test]
    fn test_that_parse_info_computes_duration_ns() {
        // GIVEN — an Info body with TimestampScale=1_000_000 and Duration=5000.0 ticks
        let data = [
            uint_element(elements::TIMESTAMP_SCALE, 1_000_000),
            float_element(elements::DURATION, 5000.0),
        ]
        .concat();

        // WHEN — we parse the info
        let info = parse_info(&data, 0).unwrap();

        // THEN — duration_ns = 5000.0 * 1_000_000 = 5_000_000_000 ns (5 seconds)
        let duration = info.duration_ns.unwrap();
        assert!((duration - 5_000_000_000.0).abs() < 1.0);
    }

    // -----------------------------------------------------------------------
    // parse_tracks
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_parse_tracks_returns_empty_for_no_entries() {
        // GIVEN — an empty Tracks body
        let data: &[u8] = &[];

        // WHEN — we parse tracks
        let tracks = parse_tracks(data, 0).unwrap();

        // THEN — the result should be empty
        assert!(tracks.is_empty());
    }

    #[test]
    fn test_that_parse_tracks_parses_single_video_track() {
        // GIVEN — a Tracks body with one VP9 video TrackEntry
        let video_settings = [
            uint_element(elements::PIXEL_WIDTH, 1920),
            uint_element(elements::PIXEL_HEIGHT, 1080),
        ]
        .concat();
        let track_entry = [
            uint_element(elements::TRACK_NUMBER, 1),
            uint_element(elements::TRACK_TYPE, elements::TRACK_TYPE_VIDEO),
            string_element(elements::CODEC_ID, elements::CODEC_ID_VP9),
            element(elements::VIDEO, &video_settings),
        ]
        .concat();
        let data = element(elements::TRACK_ENTRY, &track_entry);

        // WHEN — we parse tracks
        let tracks = parse_tracks(&data, 0).unwrap();

        // THEN — there should be exactly one track
        assert_eq!(tracks.len(), 1);
    }

    #[test]
    fn test_that_parse_tracks_reads_video_dimensions() {
        // GIVEN — a Tracks body with a 1920x1080 video track
        let video_settings = [
            uint_element(elements::PIXEL_WIDTH, 1920),
            uint_element(elements::PIXEL_HEIGHT, 1080),
        ]
        .concat();
        let track_entry = [
            uint_element(elements::TRACK_NUMBER, 1),
            uint_element(elements::TRACK_TYPE, elements::TRACK_TYPE_VIDEO),
            string_element(elements::CODEC_ID, elements::CODEC_ID_VP9),
            element(elements::VIDEO, &video_settings),
        ]
        .concat();
        let data = element(elements::TRACK_ENTRY, &track_entry);

        // WHEN — we parse tracks
        let tracks = parse_tracks(&data, 0).unwrap();

        // THEN — the track should have width=1920 and height=1080
        assert_eq!(tracks[0].width, Some(1920));
    }

    #[test]
    fn test_that_parse_tracks_reads_video_height() {
        // GIVEN — a Tracks body with a 1920x1080 video track
        let video_settings = [
            uint_element(elements::PIXEL_WIDTH, 1920),
            uint_element(elements::PIXEL_HEIGHT, 1080),
        ]
        .concat();
        let track_entry = [
            uint_element(elements::TRACK_NUMBER, 1),
            uint_element(elements::TRACK_TYPE, elements::TRACK_TYPE_VIDEO),
            string_element(elements::CODEC_ID, elements::CODEC_ID_VP9),
            element(elements::VIDEO, &video_settings),
        ]
        .concat();
        let data = element(elements::TRACK_ENTRY, &track_entry);

        // WHEN — we parse tracks
        let tracks = parse_tracks(&data, 0).unwrap();

        // THEN — the track should have height=1080
        assert_eq!(tracks[0].height, Some(1080));
    }

    #[test]
    fn test_that_parse_tracks_reads_audio_sample_rate() {
        // GIVEN — a Tracks body with an Opus audio track at 48kHz
        let audio_settings = [
            float_element(elements::SAMPLING_FREQUENCY, 48000.0),
            uint_element(elements::CHANNELS, 2),
        ]
        .concat();
        let track_entry = [
            uint_element(elements::TRACK_NUMBER, 2),
            uint_element(elements::TRACK_TYPE, elements::TRACK_TYPE_AUDIO),
            string_element(elements::CODEC_ID, elements::CODEC_ID_OPUS),
            element(elements::AUDIO, &audio_settings),
        ]
        .concat();
        let data = element(elements::TRACK_ENTRY, &track_entry);

        // WHEN — we parse tracks
        let tracks = parse_tracks(&data, 0).unwrap();

        // THEN — the track should have sample_rate=48000
        let sr = tracks[0].sample_rate.unwrap();
        assert!((sr - 48000.0).abs() < 0.01);
    }

    #[test]
    fn test_that_parse_tracks_reads_audio_channels() {
        // GIVEN — a Tracks body with a stereo Opus audio track
        let audio_settings = [
            float_element(elements::SAMPLING_FREQUENCY, 48000.0),
            uint_element(elements::CHANNELS, 2),
        ]
        .concat();
        let track_entry = [
            uint_element(elements::TRACK_NUMBER, 2),
            uint_element(elements::TRACK_TYPE, elements::TRACK_TYPE_AUDIO),
            string_element(elements::CODEC_ID, elements::CODEC_ID_OPUS),
            element(elements::AUDIO, &audio_settings),
        ]
        .concat();
        let data = element(elements::TRACK_ENTRY, &track_entry);

        // WHEN — we parse tracks
        let tracks = parse_tracks(&data, 0).unwrap();

        // THEN — the track should have 2 channels
        assert_eq!(tracks[0].channels, Some(2));
    }

    #[test]
    fn test_that_parse_tracks_stores_codec_private_data() {
        // GIVEN — a TrackEntry with CodecPrivate data
        let private_data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let track_entry = [
            uint_element(elements::TRACK_NUMBER, 1),
            uint_element(elements::TRACK_TYPE, elements::TRACK_TYPE_VIDEO),
            string_element(elements::CODEC_ID, elements::CODEC_ID_VP9),
            element(elements::CODEC_PRIVATE, &private_data),
        ]
        .concat();
        let data = element(elements::TRACK_ENTRY, &track_entry);

        // WHEN — we parse tracks
        let tracks = parse_tracks(&data, 0).unwrap();

        // THEN — codec_private should contain the raw bytes
        assert_eq!(
            tracks[0].codec_private.as_deref(),
            Some(&[0xDE, 0xAD, 0xBE, 0xEF][..])
        );
    }

    #[test]
    fn test_that_parse_tracks_handles_multiple_track_entries() {
        // GIVEN — a Tracks body with video and audio TrackEntries
        let video_entry = element(
            elements::TRACK_ENTRY,
            &[
                uint_element(elements::TRACK_NUMBER, 1),
                uint_element(elements::TRACK_TYPE, elements::TRACK_TYPE_VIDEO),
                string_element(elements::CODEC_ID, elements::CODEC_ID_VP9),
            ]
            .concat(),
        );
        let audio_entry = element(
            elements::TRACK_ENTRY,
            &[
                uint_element(elements::TRACK_NUMBER, 2),
                uint_element(elements::TRACK_TYPE, elements::TRACK_TYPE_AUDIO),
                string_element(elements::CODEC_ID, elements::CODEC_ID_OPUS),
            ]
            .concat(),
        );
        let data = [video_entry, audio_entry].concat();

        // WHEN — we parse tracks
        let tracks = parse_tracks(&data, 0).unwrap();

        // THEN — there should be two tracks
        assert_eq!(tracks.len(), 2);
    }

    #[test]
    fn test_that_parse_tracks_uses_default_audio_settings_when_absent() {
        // GIVEN — an audio TrackEntry without an Audio sub-element
        let track_entry = [
            uint_element(elements::TRACK_NUMBER, 1),
            uint_element(elements::TRACK_TYPE, elements::TRACK_TYPE_AUDIO),
            string_element(elements::CODEC_ID, elements::CODEC_ID_OPUS),
        ]
        .concat();
        let data = element(elements::TRACK_ENTRY, &track_entry);

        // WHEN — we parse tracks
        let tracks = parse_tracks(&data, 0).unwrap();

        // THEN — sample_rate and channels should be None (no Audio sub-element parsed)
        assert!(tracks[0].sample_rate.is_none());
    }

    // -----------------------------------------------------------------------
    // parse_simple_block
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_parse_simple_block_reads_track_number() {
        // GIVEN — a SimpleBlock with track_number=1, relative_ts=0, keyframe
        let mut data = Vec::new();
        data.extend_from_slice(&encode_data_size(1)); // track number vint
        data.extend_from_slice(&0i16.to_be_bytes()); // relative timestamp
        data.push(0x80); // flags: keyframe
        data.extend_from_slice(&[0xAA, 0xBB]); // frame data

        // WHEN — we parse the simple block
        let packet = parse_simple_block(&data, 0, 1_000_000, 0).unwrap().unwrap();

        // THEN — track_number should be 1
        assert_eq!(packet.track_number, 1);
    }

    #[test]
    fn test_that_parse_simple_block_detects_keyframe() {
        // GIVEN — a SimpleBlock with keyframe flag set
        let mut data = Vec::new();
        data.extend_from_slice(&encode_data_size(1));
        data.extend_from_slice(&0i16.to_be_bytes());
        data.push(0x80); // keyframe flag
        data.extend_from_slice(&[0xAA]);

        // WHEN — we parse the simple block
        let packet = parse_simple_block(&data, 0, 1_000_000, 0).unwrap().unwrap();

        // THEN — is_keyframe should be true
        assert!(packet.is_keyframe);
    }

    #[test]
    fn test_that_parse_simple_block_detects_non_keyframe() {
        // GIVEN — a SimpleBlock without keyframe flag
        let mut data = Vec::new();
        data.extend_from_slice(&encode_data_size(1));
        data.extend_from_slice(&0i16.to_be_bytes());
        data.push(0x00); // no keyframe
        data.extend_from_slice(&[0xAA]);

        // WHEN — we parse the simple block
        let packet = parse_simple_block(&data, 0, 1_000_000, 0).unwrap().unwrap();

        // THEN — is_keyframe should be false
        assert!(!packet.is_keyframe);
    }

    #[test]
    fn test_that_parse_simple_block_computes_pts_ns() {
        // GIVEN — a SimpleBlock with cluster_timestamp=1000, relative_ts=33
        let mut data = Vec::new();
        data.extend_from_slice(&encode_data_size(1));
        data.extend_from_slice(&33i16.to_be_bytes());
        data.push(0x80);
        data.extend_from_slice(&[0xAA]);

        // WHEN — we parse with cluster_timestamp=1000 and timestamp_scale=1_000_000
        let packet = parse_simple_block(&data, 1000, 1_000_000, 0)
            .unwrap()
            .unwrap();

        // THEN — pts_ns = (1000 + 33) * 1_000_000 = 1_033_000_000
        assert_eq!(packet.pts_ns, 1_033_000_000);
    }

    #[test]
    fn test_that_parse_simple_block_extracts_frame_data() {
        // GIVEN — a SimpleBlock with specific frame payload
        let mut data = Vec::new();
        data.extend_from_slice(&encode_data_size(1));
        data.extend_from_slice(&0i16.to_be_bytes());
        data.push(0x80);
        data.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        // WHEN — we parse the simple block
        let packet = parse_simple_block(&data, 0, 1_000_000, 0).unwrap().unwrap();

        // THEN — the frame data should match
        assert_eq!(&packet.data, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_that_parse_simple_block_handles_negative_relative_timestamp() {
        // GIVEN — a SimpleBlock with negative relative timestamp (-10)
        let mut data = Vec::new();
        data.extend_from_slice(&encode_data_size(1));
        data.extend_from_slice(&(-10i16).to_be_bytes());
        data.push(0x00);
        data.extend_from_slice(&[0xAA]);

        // WHEN — we parse with cluster_timestamp=100
        let packet = parse_simple_block(&data, 100, 1_000_000, 0)
            .unwrap()
            .unwrap();

        // THEN — pts_ns = (100 + (-10)) * 1_000_000 = 90_000_000
        assert_eq!(packet.pts_ns, 90_000_000);
    }

    #[test]
    fn test_that_parse_simple_block_errors_on_empty_data() {
        // GIVEN — empty data
        let data: &[u8] = &[];

        // WHEN — we parse the simple block
        let result = parse_simple_block(data, 0, 1_000_000, 0);

        // THEN — it should return an error
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // build_track_info
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_build_track_info_creates_video_track_for_vp9() {
        // GIVEN — a WebmTrack with VP9 codec
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_VP9.to_string(),
            codec_private: None,
            width: Some(1920),
            height: Some(1080),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(0, &track, None).unwrap();

        // THEN — kind should be Video
        assert_eq!(info.kind, TrackKind::Video);
    }

    #[test]
    fn test_that_build_track_info_maps_vp9_codec() {
        // GIVEN — a VP9 video track
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_VP9.to_string(),
            codec_private: None,
            width: Some(1920),
            height: Some(1080),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(0, &track, None).unwrap();

        // THEN — codec should be Video(Other("VP9"))
        assert_eq!(
            info.codec,
            Codec::Video(VideoCodec::Other("VP9".to_string()))
        );
    }

    #[test]
    fn test_that_build_track_info_maps_av1_codec() {
        // GIVEN — an AV1 video track
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_AV1.to_string(),
            codec_private: None,
            width: Some(1920),
            height: Some(1080),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(0, &track, None).unwrap();

        // THEN — codec should be Video(Av1)
        assert_eq!(info.codec, Codec::Video(VideoCodec::Av1));
    }

    #[test]
    fn test_that_build_track_info_maps_h264_codec() {
        // GIVEN — an H264 video track
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_H264.to_string(),
            codec_private: None,
            width: Some(1920),
            height: Some(1080),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(0, &track, None).unwrap();

        // THEN — codec should be Video(H264)
        assert_eq!(info.codec, Codec::Video(VideoCodec::H264));
    }

    #[test]
    fn test_that_build_track_info_maps_h265_codec() {
        // GIVEN — an H265 video track
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_H265.to_string(),
            codec_private: None,
            width: Some(1920),
            height: Some(1080),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(0, &track, None).unwrap();

        // THEN — codec should be Video(H265)
        assert_eq!(info.codec, Codec::Video(VideoCodec::H265));
    }

    #[test]
    fn test_that_build_track_info_creates_audio_track_for_opus() {
        // GIVEN — a WebmTrack with Opus codec
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_OPUS.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(2),
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — kind should be Audio
        assert_eq!(info.kind, TrackKind::Audio);
    }

    #[test]
    fn test_that_build_track_info_maps_opus_codec() {
        // GIVEN — an Opus audio track
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_OPUS.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(2),
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — codec should be Audio(Opus)
        assert_eq!(info.codec, Codec::Audio(AudioCodec::Opus));
    }

    #[test]
    fn test_that_build_track_info_maps_aac_codec() {
        // GIVEN — an AAC audio track
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_AAC.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(44100.0),
            channels: Some(2),
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — codec should be Audio(Aac)
        assert_eq!(info.codec, Codec::Audio(AudioCodec::Aac));
    }

    #[test]
    fn test_that_build_track_info_maps_vorbis_codec() {
        // GIVEN — a Vorbis audio track
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_VORBIS.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(44100.0),
            channels: Some(2),
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — codec should be Audio(Other("Vorbis"))
        assert_eq!(
            info.codec,
            Codec::Audio(AudioCodec::Other("Vorbis".to_string()))
        );
    }

    #[test]
    fn test_that_build_track_info_errors_on_unknown_video_codec() {
        // GIVEN — a video track with an unsupported codec
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: "V_UNKNOWN".to_string(),
            codec_private: None,
            width: Some(640),
            height: Some(480),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let result = build_track_info(0, &track, None);

        // THEN — it should return an UnsupportedCodec error
        assert!(result.is_err());
    }

    #[test]
    fn test_that_build_track_info_errors_on_unknown_audio_codec() {
        // GIVEN — an audio track with an unsupported codec
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: "A_UNKNOWN".to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(2),
        };

        // WHEN — we build TrackInfo
        let result = build_track_info(1, &track, None);

        // THEN — it should return an UnsupportedCodec error
        assert!(result.is_err());
    }

    #[test]
    fn test_that_build_track_info_errors_on_unknown_track_type() {
        // GIVEN — a track with an unsupported track type (subtitle = 17)
        let track = WebmTrack {
            track_number: 3,
            track_type: 17,
            codec_id: "S_TEXT/UTF8".to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let result = build_track_info(2, &track, None);

        // THEN — it should return an error
        assert!(result.is_err());
    }

    #[test]
    fn test_that_build_track_info_uses_default_dimensions_when_absent() {
        // GIVEN — a video track with no width/height set
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_VP9.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(0, &track, None).unwrap();

        // THEN — width defaults to 0
        assert_eq!(info.video.unwrap().width, 0);
    }

    #[test]
    fn test_that_build_track_info_uses_default_audio_params_when_absent() {
        // GIVEN — an audio track with no sample_rate or channels set
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_OPUS.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — sample_rate defaults to 48000
        assert_eq!(info.audio.unwrap().sample_rate, 48000);
    }

    #[test]
    fn test_that_build_track_info_maps_mono_channel_layout() {
        // GIVEN — an audio track with 1 channel
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_OPUS.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(1),
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — channel_layout should be Mono
        assert_eq!(
            info.audio.unwrap().channel_layout,
            Some(ChannelLayout::Mono)
        );
    }

    #[test]
    fn test_that_build_track_info_maps_stereo_channel_layout() {
        // GIVEN — an audio track with 2 channels
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_OPUS.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(2),
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — channel_layout should be Stereo
        assert_eq!(
            info.audio.unwrap().channel_layout,
            Some(ChannelLayout::Stereo)
        );
    }

    #[test]
    fn test_that_build_track_info_maps_surround_5_1_channel_layout() {
        // GIVEN — an audio track with 6 channels
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_OPUS.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(6),
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — channel_layout should be Surround5_1
        assert_eq!(
            info.audio.unwrap().channel_layout,
            Some(ChannelLayout::Surround5_1)
        );
    }

    #[test]
    fn test_that_build_track_info_returns_none_layout_for_unknown_channel_count() {
        // GIVEN — an audio track with 3 channels (not a standard layout)
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_OPUS.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(3),
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — channel_layout should be None
        assert!(info.audio.unwrap().channel_layout.is_none());
    }

    #[test]
    fn test_that_build_track_info_sets_duration_from_segment() {
        // GIVEN — a video track with segment duration of 10 seconds
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_VP9.to_string(),
            codec_private: None,
            width: Some(1920),
            height: Some(1080),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo with 10 seconds duration
        let info = build_track_info(0, &track, Some(10_000_000_000.0)).unwrap();

        // THEN — duration should be present
        assert!(info.duration.is_some());
    }

    #[test]
    fn test_that_build_track_info_has_no_duration_when_segment_has_none() {
        // GIVEN — a video track with no segment duration
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_VP9.to_string(),
            codec_private: None,
            width: Some(1920),
            height: Some(1080),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo without duration
        let info = build_track_info(0, &track, None).unwrap();

        // THEN — duration should be None
        assert!(info.duration.is_none());
    }

    #[test]
    fn test_that_build_track_info_sets_track_index() {
        // GIVEN — a video track built at index 3
        let track = WebmTrack {
            track_number: 4,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_VP9.to_string(),
            codec_private: None,
            width: Some(640),
            height: Some(480),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo at index 3
        let info = build_track_info(3, &track, None).unwrap();

        // THEN — the track index should be 3
        assert_eq!(info.index, TrackIndex(3));
    }

    #[test]
    fn test_that_build_track_info_video_track_has_no_audio_info() {
        // GIVEN — a video track
        let track = WebmTrack {
            track_number: 1,
            track_type: elements::TRACK_TYPE_VIDEO,
            codec_id: elements::CODEC_ID_VP9.to_string(),
            codec_private: None,
            width: Some(1920),
            height: Some(1080),
            sample_rate: None,
            channels: None,
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(0, &track, None).unwrap();

        // THEN — audio info should be None
        assert!(info.audio.is_none());
    }

    #[test]
    fn test_that_build_track_info_audio_track_has_no_video_info() {
        // GIVEN — an audio track
        let track = WebmTrack {
            track_number: 2,
            track_type: elements::TRACK_TYPE_AUDIO,
            codec_id: elements::CODEC_ID_OPUS.to_string(),
            codec_private: None,
            width: None,
            height: None,
            sample_rate: Some(48000.0),
            channels: Some(2),
        };

        // WHEN — we build TrackInfo
        let info = build_track_info(1, &track, None).unwrap();

        // THEN — video info should be None
        assert!(info.video.is_none());
    }
}
