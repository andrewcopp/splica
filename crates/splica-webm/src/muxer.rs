//! WebM muxer: writes compressed packets into a WebM (Matroska subset) container.
//!
//! Uses a streaming write model:
//! 1. Write EBML header immediately
//! 2. Start Segment with unknown size
//! 3. Buffer track info, write Info + Tracks on first packet
//! 4. Write packets into Clusters (new cluster every ~5 seconds)
//! 5. On `finalize()`, close the current cluster and update Segment duration

use std::io::{Seek, SeekFrom, Write};

use splica_core::{
    AudioCodec, Codec, MuxError, Muxer, Packet, TrackIndex, TrackInfo, TrackKind, VideoCodec,
};

use crate::ebml;
use crate::elements;

/// Maximum cluster duration in milliseconds before starting a new one.
const MAX_CLUSTER_DURATION_MS: u64 = 5000;

/// Timestamp scale: 1 millisecond in nanoseconds.
const TIMESTAMP_SCALE_NS: u64 = 1_000_000;

/// Collected metadata for one track during muxing.
struct MuxTrack {
    info: TrackInfo,
    codec_id: String,
}

/// A WebM muxer that writes to any `Write + Seek` destination.
pub struct WebmMuxer<W> {
    writer: W,
    tracks: Vec<MuxTrack>,
    /// Whether the EBML header + Segment header have been written.
    header_written: bool,
    /// File offset where the Segment body starts (after the unknown-size header).
    segment_body_start: u64,
    /// File offset of the Duration float element's data bytes (for patching).
    duration_data_offset: Option<u64>,
    /// Current cluster state.
    cluster: Option<ClusterState>,
    /// Maximum PTS seen (in milliseconds) for duration calculation.
    max_pts_ms: u64,
}

struct ClusterState {
    /// Cluster base timestamp in milliseconds.
    timestamp_ms: u64,
    /// File offset where the cluster element starts.
    start_offset: u64,
}

impl<W: Write + Seek> WebmMuxer<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            tracks: Vec::new(),
            header_written: false,
            segment_body_start: 0,
            duration_data_offset: None,
            cluster: None,
            max_pts_ms: 0,
        }
    }

    /// Writes the EBML header, Segment header, Info, and Tracks elements.
    fn write_header(&mut self) -> Result<(), MuxError> {
        // EBML Header
        let ebml_body = [
            ebml::uint_element(elements::EBML_VERSION, 1),
            ebml::uint_element(elements::EBML_READ_VERSION, 1),
            ebml::uint_element(elements::EBML_MAX_ID_LENGTH, 4),
            ebml::uint_element(elements::EBML_MAX_SIZE_LENGTH, 8),
            ebml::string_element(elements::EBML_DOC_TYPE, "webm"),
            ebml::uint_element(elements::EBML_DOC_TYPE_VERSION, 4),
            ebml::uint_element(elements::EBML_DOC_TYPE_READ_VERSION, 2),
        ]
        .concat();
        self.write_bytes(&ebml::build_element(elements::EBML, &ebml_body))?;

        // Segment with unknown size (0x01 0xFF_FFFF_FFFF_FFFF)
        let segment_id = ebml::encode_element_id(elements::SEGMENT);
        self.write_bytes(&segment_id)?;
        // Unknown size: 8-byte vint with all data bits set
        self.write_bytes(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])?;
        self.segment_body_start = self.writer.stream_position().map_err(io_err)?;

        // Info element
        let info_body = self.build_info()?;
        self.write_bytes(&ebml::build_element(elements::INFO, &info_body))?;

        // Tracks element
        let tracks_body = self.build_tracks()?;
        self.write_bytes(&ebml::build_element(elements::TRACKS, &tracks_body))?;

        self.header_written = true;
        Ok(())
    }

    fn build_info(&mut self) -> Result<Vec<u8>, MuxError> {
        let mut body = Vec::new();
        body.extend_from_slice(&ebml::uint_element(
            elements::TIMESTAMP_SCALE,
            TIMESTAMP_SCALE_NS,
        ));

        // Duration: write a placeholder 8-byte float (0.0), record offset for patching
        let duration_element_header = [
            ebml::encode_element_id(elements::DURATION),
            ebml::encode_data_size(8),
        ]
        .concat();
        body.extend_from_slice(&duration_element_header);

        // We need the absolute file offset of the duration data bytes.
        // It will be: segment_body_start + Info header size + body.len() at this point.
        // But we don't know the Info header size yet. So record relative offset and fix later.
        let duration_relative_offset = body.len();
        body.extend_from_slice(&0.0f64.to_be_bytes());

        body.extend_from_slice(&ebml::string_element(elements::MUXING_APP, "splica"));
        body.extend_from_slice(&ebml::string_element(elements::WRITING_APP, "splica"));

        // Now compute the absolute offset of the duration data.
        // The Info element will be written at the current writer position.
        // Info header = element_id(INFO) + data_size(body.len())
        let info_header_size = ebml::encode_element_id(elements::INFO).len()
            + ebml::encode_data_size(body.len() as u64).len();
        let current_pos = self.writer.stream_position().map_err(io_err)?;
        self.duration_data_offset =
            Some(current_pos + info_header_size as u64 + duration_relative_offset as u64);

        Ok(body)
    }

    fn build_tracks(&self) -> Result<Vec<u8>, MuxError> {
        let mut body = Vec::new();
        for (i, track) in self.tracks.iter().enumerate() {
            let entry = self.build_track_entry(i, track)?;
            body.extend_from_slice(&ebml::build_element(elements::TRACK_ENTRY, &entry));
        }
        Ok(body)
    }

    fn build_track_entry(&self, index: usize, track: &MuxTrack) -> Result<Vec<u8>, MuxError> {
        let track_number = (index + 1) as u64; // 1-based
        let mut entry = Vec::new();

        entry.extend_from_slice(&ebml::uint_element(elements::TRACK_NUMBER, track_number));
        entry.extend_from_slice(&ebml::uint_element(elements::TRACK_UID, track_number));
        entry.extend_from_slice(&ebml::uint_element(
            elements::FLAG_LACING,
            0, // no lacing
        ));
        entry.extend_from_slice(&ebml::string_element(elements::CODEC_ID, &track.codec_id));

        match track.info.kind {
            TrackKind::Video => {
                entry.extend_from_slice(&ebml::uint_element(
                    elements::TRACK_TYPE,
                    elements::TRACK_TYPE_VIDEO,
                ));
                if let Some(ref video) = track.info.video {
                    let video_body = [
                        ebml::uint_element(elements::PIXEL_WIDTH, video.width as u64),
                        ebml::uint_element(elements::PIXEL_HEIGHT, video.height as u64),
                    ]
                    .concat();
                    entry.extend_from_slice(&ebml::build_element(elements::VIDEO, &video_body));
                }
            }
            TrackKind::Audio => {
                entry.extend_from_slice(&ebml::uint_element(
                    elements::TRACK_TYPE,
                    elements::TRACK_TYPE_AUDIO,
                ));
                if let Some(ref audio) = track.info.audio {
                    let mut audio_body = Vec::new();
                    audio_body.extend_from_slice(&ebml::float_element(
                        elements::SAMPLING_FREQUENCY,
                        audio.sample_rate as f64,
                    ));
                    let channels = audio
                        .channel_layout
                        .map(|cl| cl.channel_count())
                        .unwrap_or(2) as u64;
                    audio_body.extend_from_slice(&ebml::uint_element(elements::CHANNELS, channels));
                    entry.extend_from_slice(&ebml::build_element(elements::AUDIO, &audio_body));
                }
            }
        }

        Ok(entry)
    }

    /// Starts a new cluster at the given timestamp.
    fn start_cluster(&mut self, timestamp_ms: u64) -> Result<(), MuxError> {
        let start_offset = self.writer.stream_position().map_err(io_err)?;

        // Write Cluster element with unknown size
        let cluster_id = ebml::encode_element_id(elements::CLUSTER);
        self.write_bytes(&cluster_id)?;
        self.write_bytes(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])?;

        // Write ClusterTimestamp
        self.write_bytes(&ebml::uint_element(
            elements::CLUSTER_TIMESTAMP,
            timestamp_ms,
        ))?;

        self.cluster = Some(ClusterState {
            timestamp_ms,
            start_offset,
        });

        Ok(())
    }

    /// Closes the current cluster by patching its size.
    fn close_cluster(&mut self) -> Result<(), MuxError> {
        if let Some(cluster) = self.cluster.take() {
            let current_pos = self.writer.stream_position().map_err(io_err)?;
            let id_len = ebml::encode_element_id(elements::CLUSTER).len() as u64;
            let size_len = 8u64; // unknown-size vint is 8 bytes
            let body_size = current_pos - cluster.start_offset - id_len - size_len;

            // Seek back and write the actual size
            self.writer
                .seek(SeekFrom::Start(cluster.start_offset + id_len))
                .map_err(io_err)?;
            self.write_bytes(&encode_data_size_8byte(body_size))?;

            // Seek back to end
            self.writer
                .seek(SeekFrom::Start(current_pos))
                .map_err(io_err)?;
        }
        Ok(())
    }

    /// Writes a SimpleBlock for the given packet.
    fn write_simple_block(
        &mut self,
        packet: &Packet,
        track_number: u64,
        cluster_timestamp_ms: u64,
    ) -> Result<(), MuxError> {
        let pts_ms = packet.pts.as_seconds_f64() * 1000.0;
        let relative_ts = (pts_ms as i64 - cluster_timestamp_ms as i64).clamp(-32768, 32767) as i16;
        let flags: u8 = if packet.is_keyframe { 0x80 } else { 0x00 };

        let track_vint = ebml::encode_data_size(track_number);

        let mut block_body = Vec::with_capacity(track_vint.len() + 3 + packet.data.len());
        block_body.extend_from_slice(&track_vint);
        block_body.extend_from_slice(&relative_ts.to_be_bytes());
        block_body.push(flags);
        block_body.extend_from_slice(&packet.data);

        self.write_bytes(&ebml::build_element(elements::SIMPLE_BLOCK, &block_body))?;

        Ok(())
    }

    fn write_bytes(&mut self, data: &[u8]) -> Result<(), MuxError> {
        self.writer.write_all(data).map_err(io_err)
    }
}

/// Encodes a data size as an 8-byte vint (for patching unknown-size fields).
fn encode_data_size_8byte(size: u64) -> [u8; 8] {
    let val = size | (1u64 << 56); // 8-byte marker: 0x01 in the high byte
    [
        (val >> 56) as u8,
        (val >> 48) as u8,
        (val >> 40) as u8,
        (val >> 32) as u8,
        (val >> 24) as u8,
        (val >> 16) as u8,
        (val >> 8) as u8,
        val as u8,
    ]
}

fn io_err(e: std::io::Error) -> MuxError {
    MuxError::Io(e)
}

fn codec_to_webm_id(codec: &Codec) -> Result<String, MuxError> {
    match codec {
        Codec::Video(VideoCodec::H264) => Ok(elements::CODEC_ID_H264.to_string()),
        Codec::Video(VideoCodec::H265) => Ok(elements::CODEC_ID_H265.to_string()),
        Codec::Video(VideoCodec::Av1) => Ok(elements::CODEC_ID_AV1.to_string()),
        Codec::Video(VideoCodec::Other(s)) => match s.as_str() {
            "VP8" => Ok(elements::CODEC_ID_VP8.to_string()),
            "VP9" => Ok(elements::CODEC_ID_VP9.to_string()),
            other => Err(MuxError::IncompatibleCodec {
                codec: other.to_string(),
                container: "WebM".to_string(),
            }),
        },
        Codec::Audio(AudioCodec::Opus) => Ok(elements::CODEC_ID_OPUS.to_string()),
        Codec::Audio(AudioCodec::Aac) => Ok(elements::CODEC_ID_AAC.to_string()),
        Codec::Audio(AudioCodec::Other(s)) => match s.as_str() {
            "Vorbis" => Ok(elements::CODEC_ID_VORBIS.to_string()),
            other => Err(MuxError::IncompatibleCodec {
                codec: other.to_string(),
                container: "WebM".to_string(),
            }),
        },
    }
}

impl<W: Write + Seek> Muxer for WebmMuxer<W> {
    fn add_track(&mut self, info: &TrackInfo) -> Result<TrackIndex, MuxError> {
        if self.header_written {
            return Err(MuxError::InvalidTrackConfig {
                message: "cannot add tracks after writing has started".to_string(),
            });
        }

        let codec_id = codec_to_webm_id(&info.codec)?;
        let index = self.tracks.len() as u32;
        self.tracks.push(MuxTrack {
            info: info.clone(),
            codec_id,
        });
        Ok(TrackIndex(index))
    }

    fn write_packet(&mut self, packet: &Packet) -> Result<(), MuxError> {
        if !self.header_written {
            if self.tracks.is_empty() {
                return Err(MuxError::InvalidTrackConfig {
                    message: "no tracks added before writing packets".to_string(),
                });
            }
            self.write_header()?;
        }

        let track_index = packet.track_index.0 as usize;
        if track_index >= self.tracks.len() {
            return Err(MuxError::InvalidTrackConfig {
                message: format!("track index {} out of range", track_index),
            });
        }

        let pts_ms = (packet.pts.as_seconds_f64() * 1000.0) as u64;

        // Track max PTS for duration
        if pts_ms > self.max_pts_ms {
            self.max_pts_ms = pts_ms;
        }

        // Determine if we need a new cluster
        let need_new_cluster = match &self.cluster {
            None => true,
            Some(cluster) => {
                pts_ms.saturating_sub(cluster.timestamp_ms) >= MAX_CLUSTER_DURATION_MS
                    && packet.is_keyframe
            }
        };

        if need_new_cluster {
            self.close_cluster()?;
            self.start_cluster(pts_ms)?;
        }

        // Cluster is guaranteed to exist: either it existed before or
        // start_cluster() was just called above in the need_new_cluster branch
        let cluster_timestamp_ms = match self.cluster.as_ref() {
            Some(c) => c.timestamp_ms,
            None => {
                return Err(MuxError::InvalidTrackConfig {
                    message: "no active cluster when writing packet".to_string(),
                });
            }
        };
        let track_number = (track_index + 1) as u64; // 1-based
        self.write_simple_block(packet, track_number, cluster_timestamp_ms)?;

        Ok(())
    }

    fn finalize(&mut self) -> Result<(), MuxError> {
        if !self.header_written {
            // Nothing was written — write header anyway for a valid but empty file
            if !self.tracks.is_empty() {
                self.write_header()?;
            }
        }

        self.close_cluster()?;

        // Patch the Duration in Info
        if let Some(offset) = self.duration_data_offset {
            let duration_ms = self.max_pts_ms as f64;
            self.writer.seek(SeekFrom::Start(offset)).map_err(io_err)?;
            self.write_bytes(&duration_ms.to_be_bytes())?;
        }

        self.writer.flush().map_err(io_err)?;
        Ok(())
    }
}
