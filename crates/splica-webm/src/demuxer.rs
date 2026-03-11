//! WebM demuxer: reads a WebM container and yields compressed packets.
//!
//! Parses the EBML header, Segment/Info/Tracks elements to build track
//! metadata, then iterates over Cluster/SimpleBlock elements to yield packets.

use std::io::{Read, Seek, SeekFrom};

use bytes::Bytes;
use splica_core::{
    AudioCodec, AudioTrackInfo, ChannelLayout, Codec, DemuxError, Demuxer, Packet, Timestamp,
    TrackIndex, TrackInfo, TrackKind, VideoCodec, VideoTrackInfo,
};

use crate::ebml;
use crate::elements;
use crate::error::WebmError;

/// Internal track metadata parsed from the Tracks element.
struct WebmTrack {
    /// Matroska track number (1-based, from TrackNumber element).
    track_number: u64,
    /// Track type: video (1) or audio (2).
    track_type: u64,
    /// Codec identifier string (e.g., "V_VP9", "A_OPUS").
    codec_id: String,
    /// Optional codec private data (used for codec initialization).
    codec_private: Option<Vec<u8>>,
    /// Video dimensions (if video track).
    width: Option<u32>,
    height: Option<u32>,
    /// Audio parameters (if audio track).
    sample_rate: Option<f64>,
    channels: Option<u64>,
}

/// A buffered packet ready to be yielded.
struct BufferedPacket {
    track_number: u64,
    pts_ns: i64,
    is_keyframe: bool,
    data: Vec<u8>,
}

/// A WebM demuxer that reads from any `Read + Seek` source.
pub struct WebmDemuxer<R> {
    reader: R,
    tracks: Vec<TrackInfo>,
    webm_tracks: Vec<WebmTrack>,
    /// TimestampScale from the Info element (nanoseconds per tick, default 1_000_000).
    timestamp_scale: u64,
    /// Current cluster timestamp in scaled ticks.
    cluster_timestamp: u64,
    /// File offset where Clusters begin (after Tracks).
    #[allow(dead_code)]
    cluster_start: u64,
    /// Current read position in the file.
    position: u64,
    /// File size.
    file_size: u64,
    /// Whether we've reached the end of the file.
    eof: bool,
    /// Buffered packets from the current cluster.
    packet_buffer: Vec<BufferedPacket>,
    /// Index into packet_buffer for the next packet to yield.
    buffer_pos: usize,
}

impl<R: Read + Seek> WebmDemuxer<R> {
    /// Opens a WebM file and parses only its metadata (EBML header, Info, Tracks).
    ///
    /// Clusters are not read into memory — they are read on-demand during
    /// `read_packet()` calls. This keeps memory usage bounded regardless of
    /// file size.
    pub fn open(mut reader: R) -> Result<Self, WebmError> {
        let file_size = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;

        // Parse EBML header
        let (ebml_header, _) = read_element_header_from_reader(&mut reader, 0)?;
        if ebml_header.id != elements::EBML {
            return Err(WebmError::NotWebm);
        }
        let ebml_size = ebml_header.data_size.ok_or(WebmError::NotWebm)? as usize;

        // Read EBML header body to verify docType
        let mut ebml_body = vec![0u8; ebml_size];
        reader.read_exact(&mut ebml_body)?;
        let ebml_body_offset = ebml_header.header_size as u64;

        let doc_type = parse_ebml_doc_type(&ebml_body, ebml_body_offset)?;
        if doc_type != "webm" && doc_type != "matroska" {
            return Err(WebmError::NotWebm);
        }

        let segment_start = ebml_header.header_size as u64 + ebml_size as u64;

        // Parse Segment header
        reader.seek(SeekFrom::Start(segment_start))?;
        let (segment_header, _) = read_element_header_from_reader(&mut reader, segment_start)?;
        if segment_header.id != elements::SEGMENT {
            return Err(WebmError::MissingElement { name: "Segment" });
        }
        let segment_body_start = segment_start + segment_header.header_size as u64;
        let segment_end = match segment_header.data_size {
            Some(size) => segment_body_start + size,
            None => file_size,
        };

        // Iterate segment children: read only Info and Tracks bodies, skip the rest
        let mut timestamp_scale: u64 = 1_000_000;
        let mut webm_tracks: Vec<WebmTrack> = Vec::new();
        let mut cluster_start: u64 = 0;

        let mut child_pos = segment_body_start;
        reader.seek(SeekFrom::Start(child_pos))?;

        while child_pos < segment_end && child_pos < file_size {
            let (child_header, header_bytes_read) =
                match read_element_header_from_reader(&mut reader, child_pos) {
                    Ok(h) => h,
                    Err(_) => break,
                };

            let child_body_offset = child_pos + header_bytes_read as u64;
            let child_size = match child_header.data_size {
                Some(s) => s,
                None => break,
            };

            match child_header.id {
                elements::INFO => {
                    let mut body = vec![0u8; child_size as usize];
                    reader.read_exact(&mut body)?;
                    timestamp_scale = parse_info(&body, child_body_offset)?;
                }
                elements::TRACKS => {
                    let mut body = vec![0u8; child_size as usize];
                    reader.read_exact(&mut body)?;
                    webm_tracks = parse_tracks(&body, child_body_offset)?;
                }
                elements::CLUSTER => {
                    cluster_start = child_pos;
                    break;
                }
                _ => {
                    // Skip unknown elements (SeekHead, Cues, etc.)
                    reader.seek(SeekFrom::Start(child_body_offset + child_size))?;
                }
            }

            child_pos = child_body_offset + child_size;
        }

        if webm_tracks.is_empty() {
            return Err(WebmError::MissingElement { name: "Tracks" });
        }

        // Build TrackInfo for each track
        let tracks: Vec<TrackInfo> = webm_tracks
            .iter()
            .enumerate()
            .map(|(i, t)| build_track_info(i, t))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            reader,
            tracks,
            webm_tracks,
            timestamp_scale,
            cluster_timestamp: 0,
            cluster_start,
            position: cluster_start,
            file_size,
            eof: cluster_start == 0,
            packet_buffer: Vec::new(),
            buffer_pos: 0,
        })
    }

    /// Reads the next cluster and buffers its SimpleBlock packets.
    fn read_next_cluster(&mut self) -> Result<bool, WebmError> {
        self.packet_buffer.clear();
        self.buffer_pos = 0;

        // Read file data at current position
        self.reader.seek(SeekFrom::Start(self.position))?;

        // Read enough for a header
        let mut header_buf = [0u8; 12];
        let bytes_read = self.reader.read(&mut header_buf)?;
        if bytes_read < 2 {
            self.eof = true;
            return Ok(false);
        }

        let header = match ebml::parse_element_header(&header_buf[..bytes_read], self.position) {
            Ok(h) => h,
            Err(_) => {
                self.eof = true;
                return Ok(false);
            }
        };

        if header.id != elements::CLUSTER {
            // Skip non-cluster elements
            let total_size = header.header_size as u64 + header.data_size.unwrap_or(0);
            self.position += total_size;
            if self.position >= self.file_size {
                self.eof = true;
                return Ok(false);
            }
            return self.read_next_cluster();
        }

        let cluster_size = match header.data_size {
            Some(s) => s,
            None => {
                self.eof = true;
                return Ok(false);
            }
        };

        let cluster_body_start = self.position + header.header_size as u64;
        let cluster_end = cluster_body_start + cluster_size;

        // Read the entire cluster body
        self.reader.seek(SeekFrom::Start(cluster_body_start))?;
        let mut cluster_data = vec![0u8; cluster_size as usize];
        self.reader.read_exact(&mut cluster_data)?;

        // Parse cluster children
        self.cluster_timestamp = 0;
        let mut child_pos: usize = 0;

        while child_pos < cluster_data.len() {
            let child_header = match ebml::parse_element_header(
                &cluster_data[child_pos..],
                cluster_body_start + child_pos as u64,
            ) {
                Ok(h) => h,
                Err(_) => break,
            };

            let child_body_start = child_pos + child_header.header_size;
            let child_size = match child_header.data_size {
                Some(s) => s as usize,
                None => break,
            };

            if child_body_start + child_size > cluster_data.len() {
                break;
            }

            let child_body = &cluster_data[child_body_start..child_body_start + child_size];

            match child_header.id {
                elements::CLUSTER_TIMESTAMP => {
                    self.cluster_timestamp = ebml::read_uint(child_body, self.position)?;
                }
                elements::SIMPLE_BLOCK => {
                    if let Some(pkt) = parse_simple_block(
                        child_body,
                        self.cluster_timestamp,
                        self.timestamp_scale,
                        cluster_body_start + child_body_start as u64,
                    )? {
                        self.packet_buffer.push(pkt);
                    }
                }
                _ => {
                    // Skip BlockGroup, etc. for now
                }
            }

            child_pos = child_body_start + child_size;
        }

        self.position = cluster_end;
        if self.position >= self.file_size {
            self.eof = true;
        }

        Ok(!self.packet_buffer.is_empty())
    }

    /// Resolves a Matroska track number to a splica TrackIndex.
    fn resolve_track(&self, track_number: u64) -> Option<TrackIndex> {
        self.webm_tracks
            .iter()
            .position(|t| t.track_number == track_number)
            .map(|i| TrackIndex(i as u32))
    }

    /// Returns the codec private data for a given track index, if present.
    pub fn codec_private(&self, track: TrackIndex) -> Option<&[u8]> {
        self.webm_tracks
            .get(track.0 as usize)
            .and_then(|t| t.codec_private.as_deref())
    }
}

impl<R: Read + Seek> Demuxer for WebmDemuxer<R> {
    fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    fn read_packet(&mut self) -> Result<Option<Packet>, DemuxError> {
        loop {
            // Yield from buffer if available
            if self.buffer_pos < self.packet_buffer.len() {
                let buffered = &self.packet_buffer[self.buffer_pos];
                self.buffer_pos += 1;

                let track_index = match self.resolve_track(buffered.track_number) {
                    Some(idx) => idx,
                    None => continue, // Skip packets for unknown tracks
                };

                // Convert nanosecond timestamp to our Timestamp type
                // Use nanosecond timebase (1_000_000_000)
                let timebase = 1_000_000_000u32;
                let pts = Timestamp::new(buffered.pts_ns, timebase).ok_or_else(|| {
                    DemuxError::InvalidContainer {
                        offset: 0,
                        message: "nanosecond timebase is zero".to_string(),
                    }
                })?;

                return Ok(Some(Packet {
                    track_index,
                    pts,
                    dts: pts, // WebM SimpleBlock doesn't have separate DTS
                    is_keyframe: buffered.is_keyframe,
                    data: Bytes::copy_from_slice(&buffered.data),
                }));
            }

            // Try to read more clusters
            if self.eof {
                return Ok(None);
            }

            match self.read_next_cluster() {
                Ok(true) => continue,
                Ok(false) => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Reads an EBML element header directly from a reader without buffering the
/// entire file. Seeks the reader to just after the header (start of body).
/// Returns the parsed header and the number of bytes consumed.
fn read_element_header_from_reader<R: Read + Seek>(
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

fn parse_ebml_doc_type(data: &[u8], base_offset: u64) -> Result<String, WebmError> {
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

fn parse_info(data: &[u8], base_offset: u64) -> Result<u64, WebmError> {
    let mut timestamp_scale: u64 = 1_000_000;
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

        if header.id == elements::TIMESTAMP_SCALE {
            timestamp_scale = ebml::read_uint(&data[body_start..body_start + size], base_offset)?;
        }

        pos = body_start + size;
    }

    Ok(timestamp_scale)
}

fn parse_tracks(data: &[u8], base_offset: u64) -> Result<Vec<WebmTrack>, WebmError> {
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
fn parse_simple_block(
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

fn build_track_info(index: usize, track: &WebmTrack) -> Result<TrackInfo, WebmError> {
    let (kind, codec) = match track.track_type {
        elements::TRACK_TYPE_VIDEO => {
            let video_codec = match track.codec_id.as_str() {
                elements::CODEC_ID_VP8 => VideoCodec::Other("VP8".to_string()),
                elements::CODEC_ID_VP9 => VideoCodec::Other("VP9".to_string()),
                elements::CODEC_ID_AV1 => VideoCodec::Av1,
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

    Ok(TrackInfo {
        index: TrackIndex(index as u32),
        kind,
        codec,
        duration: None, // WebM duration is in the Info element, not per-track
        video,
        audio,
    })
}
