//! WebM demuxer: reads a WebM container and yields compressed packets.
//!
//! Parses the EBML header, Segment/Info/Tracks elements to build track
//! metadata, then iterates over Cluster/SimpleBlock elements to yield packets.

mod parsing;

use std::io::{Read, Seek, SeekFrom};

use bytes::Bytes;
use splica_core::{
    DemuxError, Demuxer, Packet, SeekMode, Seekable, Timestamp, TrackIndex, TrackInfo,
};

use crate::ebml;
use crate::elements;
use crate::error::WebmError;

use parsing::{
    build_track_info, parse_ebml_doc_type, parse_info, parse_simple_block, parse_tracks,
    read_element_header_from_reader, BufferedPacket, SegmentInfo, WebmTrack,
};

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
        let mut segment_info = SegmentInfo {
            timestamp_scale: 1_000_000,
            duration_ns: None,
        };
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
                    segment_info = parse_info(&body, child_body_offset)?;
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

        let timestamp_scale = segment_info.timestamp_scale;

        // Build TrackInfo for each track
        let tracks: Vec<TrackInfo> = webm_tracks
            .iter()
            .enumerate()
            .map(|(i, t)| build_track_info(i, t, segment_info.duration_ns))
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

    /// Returns the presentation timestamp of the current read position.
    ///
    /// After a seek, this returns the timestamp of the packet that will be
    /// yielded by the next `read_packet()` call. Returns `None` if the
    /// demuxer is at end-of-stream or the buffer is empty.
    pub fn seek_position(&self) -> Option<Timestamp> {
        if self.buffer_pos < self.packet_buffer.len() {
            let pkt = &self.packet_buffer[self.buffer_pos];
            Timestamp::new(pkt.pts_ns, 1_000_000_000)
        } else {
            None
        }
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

impl<R: Read + Seek> Seekable for WebmDemuxer<R> {
    fn seek(&mut self, target: Timestamp, mode: SeekMode) -> Result<(), DemuxError> {
        let target_ns = (target.as_seconds_f64() * 1_000_000_000.0) as i64;

        // Reset to beginning of clusters
        self.position = self.cluster_start;
        self.eof = false;
        self.packet_buffer.clear();
        self.buffer_pos = 0;

        // Track the best match: (cluster file offset, index within cluster buffer)
        let mut best: Option<(u64, usize)> = None;

        loop {
            let cluster_pos = self.position;

            if !self
                .read_next_cluster()
                .map_err(|e| -> DemuxError { e.into() })?
            {
                break;
            }

            let mut found_past_target = false;

            for (i, pkt) in self.packet_buffer.iter().enumerate() {
                let is_candidate = match mode {
                    SeekMode::Keyframe => pkt.is_keyframe && pkt.pts_ns <= target_ns,
                    SeekMode::Precise => pkt.pts_ns <= target_ns,
                };
                if is_candidate {
                    best = Some((cluster_pos, i));
                }
                if pkt.pts_ns > target_ns {
                    found_past_target = true;
                }
            }

            if found_past_target {
                break;
            }
        }

        // Re-read the cluster containing the best match and position within it
        if let Some((cluster_pos, buffer_idx)) = best {
            self.position = cluster_pos;
            self.eof = false;
            let _ = self
                .read_next_cluster()
                .map_err(|e| -> DemuxError { e.into() })?;
            self.buffer_pos = buffer_idx;
        } else {
            // No suitable packet found — reset to start of clusters
            self.position = self.cluster_start;
            self.eof = false;
            self.packet_buffer.clear();
            self.buffer_pos = 0;
        }

        Ok(())
    }
}
