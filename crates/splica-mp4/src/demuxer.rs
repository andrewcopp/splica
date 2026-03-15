//! MP4 demuxer: reads an MP4 container and yields compressed packets.

use std::io::{Read, Seek, SeekFrom};

use bytes::Bytes;
use splica_core::{
    DemuxError, Demuxer, Packet, ResourceBudget, SeekMode, Seekable, Timestamp, TrackIndex,
    TrackInfo,
};

use crate::boxes::{self, mvhd, require_box, stsd, FourCC};
use crate::error::Mp4Error;
use crate::metadata::MetadataBox;
use crate::track::Mp4Track;
use crate::track_parsing::parse_track;

/// An MP4 demuxer that reads from any `Read + Seek` source.
pub struct Mp4Demuxer<R> {
    reader: R,
    tracks: Vec<TrackInfo>,
    mp4_tracks: Vec<Mp4Track>,
    /// Interleaved read order: (track_index, sample_index_within_track)
    read_order: Vec<(usize, usize)>,
    /// Current position in the read order.
    position: usize,
    /// Optional resource limits.
    budget: Option<ResourceBudget>,
    /// Running count of bytes read from sample data.
    bytes_read: u64,
    /// Running count of packets read.
    packets_read: u64,
    /// Opaque metadata boxes (udta, meta) from moov, preserved for passthrough.
    metadata_boxes: Vec<MetadataBox>,
}

impl<R: Read + Seek> Mp4Demuxer<R> {
    /// Opens an MP4 file and parses its metadata.
    ///
    /// Reads the `moov` box into memory and constructs sample tables for
    /// all tracks. The reader is then used for on-demand sample data reads.
    pub fn open(reader: R) -> Result<Self, Mp4Error> {
        Self::open_with_budget(reader, None)
    }

    /// Returns opaque metadata boxes (udta, meta) extracted from the moov container.
    ///
    /// These can be passed to `Mp4Muxer::set_metadata` for lossless round-tripping.
    pub fn metadata(&self) -> &[MetadataBox] {
        &self.metadata_boxes
    }

    /// Returns the codec configuration for a given track index.
    ///
    /// Needed for stream-copy muxing where the muxer needs the raw codec
    /// configuration (avcC, esds, etc.) to write sample description boxes.
    pub fn codec_config(&self, track: TrackIndex) -> Option<&stsd::CodecConfig> {
        self.mp4_tracks
            .get(track.0 as usize)
            .map(|t| &t.codec_config)
    }

    /// Returns the media timescale for a given track index.
    pub fn track_timescale(&self, track: TrackIndex) -> Option<u32> {
        self.mp4_tracks.get(track.0 as usize).map(|t| t.timescale)
    }

    /// Returns the presentation timestamp of the current read position.
    ///
    /// After a seek, this returns the timestamp of the packet that will be
    /// yielded by the next `read_packet()` call. Returns `None` if the
    /// demuxer is at end-of-stream.
    pub fn seek_position(&self) -> Option<Timestamp> {
        if self.position >= self.read_order.len() {
            return None;
        }
        let (track_idx, sample_idx) = self.read_order[self.position];
        let track = &self.mp4_tracks[track_idx];
        let sample = &track.sample_table.entries[sample_idx];
        let pts_ticks = sample.dts + sample.cts_offset as i64;
        Timestamp::new(pts_ticks, track.sample_table.timescale)
    }

    /// Opens an MP4 file with a resource budget.
    ///
    /// The budget limits how many bytes can be buffered (moov box + sample
    /// data) and optionally how many packets can be read. Exceeding either
    /// limit returns `Mp4Error::ResourceExhausted` before allocation.
    pub fn open_with_budget(
        mut reader: R,
        budget: Option<ResourceBudget>,
    ) -> Result<Self, Mp4Error> {
        // Read the entire file into memory for moov parsing.
        // The moov box is typically small (<1MB) even for long files,
        // but we need to scan top-level boxes to find it.
        let file_size = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;

        // Read top-level boxes to find ftyp and moov
        let mut ftyp_found = false;
        let mut moov_data: Option<Vec<u8>> = None;
        let mut pos: u64 = 0;

        while pos < file_size {
            reader.seek(SeekFrom::Start(pos))?;

            // Read 8-byte header first. If size == 1 (extended), read 8 more bytes.
            let mut header_buf = [0u8; 16];
            if reader.read_exact(&mut header_buf[..8]).is_err() {
                break;
            }

            // Check if this is an extended-size box (size field == 1)
            let size_field =
                u32::from_be_bytes([header_buf[0], header_buf[1], header_buf[2], header_buf[3]]);
            let header_len = if size_field == 1 {
                if reader.read_exact(&mut header_buf[8..16]).is_err() {
                    break;
                }
                16usize
            } else {
                8usize
            };

            let header = boxes::parse_box_header(&header_buf[..header_len], pos)?;

            let total_size = if header.size == 0 {
                file_size - pos
            } else {
                header.size
            };

            if total_size < header.header_size as u64 {
                return Err(Mp4Error::InvalidBox {
                    offset: pos,
                    message: format!("box '{}' size too small", header.box_type),
                });
            }

            let actual_header_size = header.header_size;

            match header.box_type {
                FourCC::FTYP => {
                    let body_size = total_size - actual_header_size as u64;
                    let mut body = vec![0u8; body_size as usize];
                    reader.read_exact(&mut body)?;
                    // Validate ftyp — just check it parses
                    let _ = crate::boxes::ftyp::parse_ftyp(&body, pos)?;
                    ftyp_found = true;
                }
                FourCC::MOOV => {
                    let body_size = total_size - actual_header_size as u64;
                    // Enforce budget before allocating the moov buffer
                    if let Some(ref b) = budget {
                        if body_size > b.max_bytes {
                            return Err(Mp4Error::ResourceExhausted {
                                message: format!(
                                    "moov box size ({body_size} bytes) exceeds budget ({} bytes)",
                                    b.max_bytes
                                ),
                            });
                        }
                    }
                    let mut body = vec![0u8; body_size as usize];
                    reader.read_exact(&mut body)?;
                    moov_data = Some(body);
                }
                _ => {
                    // Skip unknown or unneeded top-level boxes (mdat, free, etc.)
                }
            }

            pos += total_size;
        }

        if !ftyp_found {
            return Err(Mp4Error::NotMp4);
        }

        let moov = moov_data.ok_or(Mp4Error::MissingBox { name: "moov" })?;

        // Parse tracks and metadata from moov
        let mvhd_box = require_box(&moov, FourCC::MVHD, 0, "mvhd")?;
        let movie_header = mvhd::parse_mvhd(mvhd_box.body, mvhd_box.offset)?;

        let mut mp4_tracks = Vec::new();
        let mut metadata_boxes = Vec::new();
        let mut skipped_codecs: Vec<String> = Vec::new();

        for box_result in boxes::iter_boxes(&moov, 0) {
            let parsed = box_result?;
            match parsed.header.box_type {
                FourCC::TRAK => {
                    match parse_track(parsed.body, parsed.offset, &movie_header) {
                        Ok(track) => {
                            // Only keep video and audio tracks
                            if track.is_video() || track.is_audio() {
                                mp4_tracks.push(track);
                            }
                        }
                        Err(Mp4Error::UnsupportedCodec { fourcc }) => {
                            skipped_codecs.push(fourcc);
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }
                FourCC::UDTA | FourCC::META => {
                    // Preserve the complete box (header + body) as raw bytes
                    let total_size = parsed.header.size as usize;
                    let header_size = parsed.header.header_size as usize;
                    let mut raw = Vec::with_capacity(total_size);
                    // Reconstruct box header
                    raw.extend_from_slice(&(total_size as u32).to_be_bytes());
                    raw.extend_from_slice(&parsed.header.box_type.0);
                    if header_size == 16 {
                        // Extended size — shouldn't happen for small metadata, but handle it
                        raw.clear();
                        raw.extend_from_slice(&1u32.to_be_bytes());
                        raw.extend_from_slice(&parsed.header.box_type.0);
                        raw.extend_from_slice(&(total_size as u64).to_be_bytes());
                    }
                    raw.extend_from_slice(parsed.body);
                    metadata_boxes.push(MetadataBox {
                        box_type: parsed.header.box_type,
                        data: raw,
                    });
                }
                _ => {
                    // Skip mvhd (already parsed) and other boxes
                }
            }
        }

        // If all tracks were skipped due to unsupported codecs, surface the error
        // instead of returning an empty track list that fails downstream.
        if mp4_tracks.is_empty() && !skipped_codecs.is_empty() {
            return Err(Mp4Error::UnsupportedCodec {
                fourcc: skipped_codecs.join(", "),
            });
        }

        // Build TrackInfo for each track
        let tracks: Vec<TrackInfo> = mp4_tracks
            .iter()
            .enumerate()
            .map(|(i, t)| t.to_track_info(TrackIndex(i as u32)))
            .collect();

        // Build interleaved read order sorted by file offset
        let mut read_order: Vec<(usize, usize)> = Vec::new();
        for (track_idx, track) in mp4_tracks.iter().enumerate() {
            for sample_idx in 0..track.sample_table.entries.len() {
                read_order.push((track_idx, sample_idx));
            }
        }
        read_order.sort_by_key(|&(track_idx, sample_idx)| {
            mp4_tracks[track_idx].sample_table.entries[sample_idx].offset
        });

        Ok(Self {
            reader,
            tracks,
            mp4_tracks,
            read_order,
            position: 0,
            budget,
            bytes_read: 0,
            packets_read: 0,
            metadata_boxes,
        })
    }
}

impl<R: Read + Seek> Demuxer for Mp4Demuxer<R> {
    fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    fn read_packet(&mut self) -> Result<Option<Packet>, DemuxError> {
        if self.position >= self.read_order.len() {
            return Ok(None);
        }

        let (track_idx, sample_idx) = self.read_order[self.position];
        self.position += 1;

        let track = &self.mp4_tracks[track_idx];
        let sample = &track.sample_table.entries[sample_idx];

        // Enforce budget before reading sample data
        if let Some(ref b) = self.budget {
            if let Some(max_frames) = b.max_frames {
                if self.packets_read >= max_frames {
                    return Err(Mp4Error::ResourceExhausted {
                        message: format!(
                            "packet count ({}) exceeds budget ({max_frames} frames)",
                            self.packets_read
                        ),
                    }
                    .into());
                }
            }
            let new_total = self.bytes_read + sample.size as u64;
            if new_total > b.max_bytes {
                return Err(Mp4Error::ResourceExhausted {
                    message: format!(
                        "reading sample ({} bytes) would exceed byte budget ({} + {} > {})",
                        sample.size, self.bytes_read, sample.size, b.max_bytes
                    ),
                }
                .into());
            }
        }

        // Seek to sample data and read it
        self.reader
            .seek(SeekFrom::Start(sample.offset))
            .map_err(Mp4Error::Io)?;

        let mut data = vec![0u8; sample.size as usize];
        self.reader
            .read_exact(&mut data)
            .map_err(|_| -> DemuxError {
                Mp4Error::UnexpectedEof {
                    offset: sample.offset,
                }
                .into()
            })?;

        self.bytes_read += sample.size as u64;
        self.packets_read += 1;

        let dts = Timestamp::new(sample.dts, track.sample_table.timescale).ok_or_else(|| {
            DemuxError::InvalidContainer {
                offset: sample.offset,
                message: format!("track timescale is zero (track {})", track_idx),
            }
        })?;
        let pts = Timestamp::new(
            sample.dts + sample.cts_offset as i64,
            track.sample_table.timescale,
        )
        .ok_or_else(|| DemuxError::InvalidContainer {
            offset: sample.offset,
            message: format!("track timescale is zero (track {})", track_idx),
        })?;

        Ok(Some(Packet {
            track_index: TrackIndex(track_idx as u32),
            pts,
            dts,
            is_keyframe: sample.is_keyframe,
            data: Bytes::from(data),
        }))
    }
}

impl<R: Read + Seek> Seekable for Mp4Demuxer<R> {
    fn seek(&mut self, target: Timestamp, mode: SeekMode) -> Result<(), DemuxError> {
        // Find the first video track, or fall back to the first track.
        let video_track_idx = self
            .mp4_tracks
            .iter()
            .position(|t| t.is_video())
            .or(if self.mp4_tracks.is_empty() {
                None
            } else {
                Some(0)
            })
            .ok_or_else(|| DemuxError::InvalidContainer {
                offset: 0,
                message: "no tracks available for seeking".to_string(),
            })?;

        let track = &self.mp4_tracks[video_track_idx];

        if track.sample_table.entries.is_empty() {
            return Err(DemuxError::InvalidContainer {
                offset: 0,
                message: "sample table is empty, cannot seek".to_string(),
            });
        }

        let target_ticks = target
            .rescale(track.sample_table.timescale)
            .map(|t| t.ticks())
            .ok_or_else(|| DemuxError::InvalidContainer {
                offset: 0,
                message: format!(
                    "cannot rescale seek target to track timescale {}",
                    track.sample_table.timescale
                ),
            })?;

        // Find the sample at or before the target timestamp.
        // If target is before all samples, use the first sample.
        let sample_idx = track
            .sample_table
            .entries
            .iter()
            .rposition(|s| s.dts <= target_ticks)
            .unwrap_or(0);

        let seek_sample = match mode {
            SeekMode::Keyframe => {
                // Find the nearest keyframe at or before the target.
                // If no keyframe exists before the target, use the first keyframe.
                track.sample_table.entries[..=sample_idx]
                    .iter()
                    .rposition(|s| s.is_keyframe)
                    .or_else(|| {
                        track
                            .sample_table
                            .entries
                            .iter()
                            .position(|s| s.is_keyframe)
                    })
                    .ok_or_else(|| DemuxError::InvalidContainer {
                        offset: 0,
                        message: "no keyframes found in sample table".to_string(),
                    })?
            }
            SeekMode::Precise => sample_idx,
        };

        // Find this sample in the read order
        let read_pos = self
            .read_order
            .iter()
            .position(|&(ti, si)| ti == video_track_idx && si == seek_sample)
            .ok_or_else(|| DemuxError::InvalidContainer {
                offset: 0,
                message: format!("seek target sample {} not found in read order", seek_sample),
            })?;

        self.position = read_pos;
        Ok(())
    }
}
