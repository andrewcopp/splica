//! EBML parsing helpers for the WebM demuxer.
//!
//! Standalone functions that parse EBML elements (Info, Tracks, SimpleBlock, etc.)
//! into domain types. These are pure parsers with no dependency on `WebmDemuxer` state.

use std::io::{Read, Seek, SeekFrom};

use splica_core::{
    AudioCodec, AudioTrackInfo, ChannelLayout, Codec, SubtitleCodec, Timestamp, TrackIndex,
    TrackInfo, TrackKind, VideoCodec, VideoTrackInfo,
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
        elements::TRACK_TYPE_SUBTITLE => {
            let subtitle_codec = match track.codec_id.as_str() {
                elements::CODEC_ID_SRT => SubtitleCodec::Srt,
                elements::CODEC_ID_WEBVTT => SubtitleCodec::WebVtt,
                other => SubtitleCodec::Other(other.to_string()),
            };
            (TrackKind::Subtitle, Codec::Subtitle(subtitle_codec))
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
