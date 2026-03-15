//! MP4 muxer: writes compressed packets into an MP4 container.
//!
//! Uses a streaming write model:
//! 1. Write `ftyp` box immediately
//! 2. Write `mdat` header with placeholder size
//! 3. Write each packet's data as it arrives, tracking offsets
//! 4. On `finalize()`, seek back to fix `mdat` size, then write `moov` at end

use std::io::{Seek, SeekFrom, Write};

use splica_core::{
    Codec, MuxError, Muxer, Packet, ResourceBudget, TrackIndex, TrackInfo, TrackKind, VideoCodec,
};

use crate::box_builders::{
    build_dinf, build_hdlr, build_mdhd, build_mvhd, build_smhd, build_stsd, build_tkhd, build_vmhd,
    io_err, make_box, make_full_box,
};
use crate::boxes::hdlr::HandlerType;
use crate::boxes::stsd::CodecConfig;
use crate::metadata::MetadataBox;

/// A single sample's metadata recorded during muxing.
struct MuxSample {
    offset: u64,
    size: u32,
    dts: i64,
    cts_offset: i32,
    is_sync: bool,
}

/// Collected metadata for one track during muxing.
struct MuxTrack {
    track_info: TrackInfo,
    codec_config: CodecConfig,
    samples: Vec<MuxSample>,
    timescale: u32,
}

/// An MP4 muxer that writes to any `Write + Seek` destination.
pub struct Mp4Muxer<W> {
    writer: W,
    tracks: Vec<MuxTrack>,
    /// File offset where mdat body starts.
    mdat_body_start: u64,
    /// Current write position within mdat.
    mdat_pos: u64,
    /// Whether ftyp+mdat header have been written.
    header_written: bool,
    /// Optional resource limits.
    budget: Option<ResourceBudget>,
    /// Running count of bytes written to mdat.
    bytes_written: u64,
    /// Running count of packets written.
    packets_written: u64,
    /// Opaque metadata boxes to write into moov.
    metadata_boxes: Vec<MetadataBox>,
}

impl<W: Write + Seek> Mp4Muxer<W> {
    /// Creates a new MP4 muxer.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            tracks: Vec::new(),
            mdat_body_start: 0,
            mdat_pos: 0,
            header_written: false,
            budget: None,
            bytes_written: 0,
            packets_written: 0,
            metadata_boxes: Vec::new(),
        }
    }

    /// Creates a new MP4 muxer with a resource budget.
    ///
    /// The budget limits how many bytes and packets can be written.
    /// Exceeding either limit returns `MuxError::ResourceExhausted`
    /// before the write occurs.
    pub fn new_with_budget(writer: W, budget: ResourceBudget) -> Self {
        Self {
            writer,
            tracks: Vec::new(),
            mdat_body_start: 0,
            mdat_pos: 0,
            header_written: false,
            budget: Some(budget),
            bytes_written: 0,
            packets_written: 0,
            metadata_boxes: Vec::new(),
        }
    }

    /// Sets metadata boxes to include in the output moov container.
    ///
    /// Typically called with the result of `Mp4Demuxer::metadata()` for
    /// lossless metadata passthrough.
    pub fn set_metadata(&mut self, boxes: Vec<MetadataBox>) {
        self.metadata_boxes = boxes;
    }

    /// Registers a track with full codec configuration (for passthrough muxing).
    ///
    /// The `codec_config` contains raw codec-specific data (avcC, esds, etc.)
    /// needed to write the sample description box.
    pub fn add_track_with_config(
        &mut self,
        info: &TrackInfo,
        codec_config: CodecConfig,
        timescale: u32,
    ) -> Result<TrackIndex, MuxError> {
        let index = TrackIndex(self.tracks.len() as u32);
        self.tracks.push(MuxTrack {
            track_info: info.clone(),
            codec_config,
            samples: Vec::new(),
            timescale,
        });
        Ok(index)
    }

    fn ensure_header_written(&mut self) -> Result<(), MuxError> {
        if self.header_written {
            return Ok(());
        }

        // Write ftyp box
        let ftyp = build_ftyp();
        self.writer.write_all(&ftyp).map_err(io_err)?;

        // Write mdat header with placeholder size (we'll fix it in finalize)
        let mdat_header_offset = self.writer.stream_position().map_err(io_err)?;
        // Use 64-bit extended size for safety
        // Box header: size=1 (signals extended), type="mdat", extended_size=placeholder
        self.writer.write_all(&1u32.to_be_bytes()).map_err(io_err)?;
        self.writer.write_all(b"mdat").map_err(io_err)?;
        self.writer.write_all(&0u64.to_be_bytes()).map_err(io_err)?; // placeholder
        self.mdat_body_start = mdat_header_offset + 16; // 4 + 4 + 8 = 16
        self.mdat_pos = self.mdat_body_start;
        self.header_written = true;
        Ok(())
    }

    /// Writes a packet to the mdat section, recording metadata for the sample table.
    pub fn write_packet_data(&mut self, packet: &Packet) -> Result<(), MuxError> {
        self.ensure_header_written()?;

        let track_idx = packet.track_index.0 as usize;
        if track_idx >= self.tracks.len() {
            return Err(MuxError::InvalidTrackConfig {
                message: format!("track index {} out of range", track_idx),
            });
        }

        // Check budget before writing
        let size = packet.data.len() as u32;
        if let Some(budget) = &self.budget {
            if self.bytes_written + size as u64 > budget.max_bytes {
                return Err(MuxError::ResourceExhausted {
                    message: format!(
                        "writing {} bytes would exceed byte budget ({} + {} > {})",
                        size, self.bytes_written, size, budget.max_bytes
                    ),
                });
            }
            if let Some(max_frames) = budget.max_frames {
                if self.packets_written + 1 > max_frames {
                    return Err(MuxError::ResourceExhausted {
                        message: format!(
                            "writing packet would exceed frame budget ({} >= {})",
                            self.packets_written, max_frames
                        ),
                    });
                }
            }
        }

        let offset = self.mdat_pos;

        self.writer.write_all(&packet.data).map_err(io_err)?;
        self.mdat_pos += size as u64;
        self.bytes_written += size as u64;
        self.packets_written += 1;

        let track_timescale = self.tracks[track_idx].timescale;
        let dts_rescaled = packet
            .dts
            .rescale(track_timescale)
            .map(|t| t.ticks())
            .unwrap_or(packet.dts.ticks());
        let pts_rescaled = packet
            .pts
            .rescale(track_timescale)
            .map(|t| t.ticks())
            .unwrap_or(packet.pts.ticks());
        let cts_offset = (pts_rescaled - dts_rescaled) as i32;

        self.tracks[track_idx].samples.push(MuxSample {
            offset,
            size,
            dts: dts_rescaled,
            cts_offset,
            is_sync: packet.is_keyframe,
        });

        Ok(())
    }

    /// Finalizes the MP4 file: fixes mdat size and writes the moov box.
    pub fn finalize_file(&mut self) -> Result<(), MuxError> {
        self.ensure_header_written()?;

        let mdat_end = self.mdat_pos;
        let mdat_total_size = mdat_end - (self.mdat_body_start - 16);

        // Fix mdat extended size
        self.writer
            .seek(SeekFrom::Start(self.mdat_body_start - 8))
            .map_err(io_err)?;
        self.writer
            .write_all(&mdat_total_size.to_be_bytes())
            .map_err(io_err)?;

        // Seek to end for moov
        self.writer
            .seek(SeekFrom::Start(mdat_end))
            .map_err(io_err)?;

        // Build and write moov
        let moov = self.build_moov()?;
        self.writer.write_all(&moov).map_err(io_err)?;

        self.writer.flush().map_err(io_err)?;
        Ok(())
    }

    fn build_moov(&self) -> Result<Vec<u8>, MuxError> {
        let movie_timescale = 1000u32;

        // Compute movie duration
        let movie_duration = self
            .tracks
            .iter()
            .map(|t| {
                if t.samples.is_empty() || t.timescale == 0 {
                    return 0u64;
                }
                let last = &t.samples[t.samples.len() - 1];
                // Rough: last DTS + one delta, scaled to movie timescale
                let delta = if t.samples.len() > 1 {
                    (t.samples[1].dts - t.samples[0].dts).unsigned_abs()
                } else {
                    1
                };
                (last.dts as u64 + delta) * movie_timescale as u64 / t.timescale as u64
            })
            .max()
            .unwrap_or(0);

        let mvhd = build_mvhd(movie_timescale, movie_duration as u32);

        let mut trak_boxes = Vec::new();
        for (i, track) in self.tracks.iter().enumerate() {
            let trak = self.build_trak(track, i as u32 + 1, movie_timescale, movie_duration)?;
            trak_boxes.extend_from_slice(&trak);
        }

        let mut moov_body = mvhd;
        moov_body.extend_from_slice(&trak_boxes);

        // Append opaque metadata boxes (udta, meta, etc.)
        for meta_box in &self.metadata_boxes {
            moov_body.extend_from_slice(&meta_box.data);
        }

        Ok(make_box(b"moov", &moov_body))
    }

    fn build_trak(
        &self,
        track: &MuxTrack,
        track_id: u32,
        movie_timescale: u32,
        _movie_duration: u64,
    ) -> Result<Vec<u8>, MuxError> {
        let (width, height) = match &track.track_info.video {
            Some(v) => (v.width, v.height),
            None => (0, 0),
        };

        let track_dur_movie = if track.samples.is_empty() || track.timescale == 0 {
            0u32
        } else {
            let last = &track.samples[track.samples.len() - 1];
            let delta = if track.samples.len() > 1 {
                (track.samples[1].dts - track.samples[0].dts).unsigned_abs()
            } else {
                1
            };
            ((last.dts as u64 + delta) * movie_timescale as u64 / track.timescale as u64) as u32
        };

        let tkhd = build_tkhd(track_id, track_dur_movie, width, height);

        // mdia
        let media_duration = if track.samples.is_empty() {
            0u32
        } else {
            let last = &track.samples[track.samples.len() - 1];
            let delta = if track.samples.len() > 1 {
                (track.samples[1].dts - track.samples[0].dts).unsigned_abs()
            } else {
                1
            };
            (last.dts as u64 + delta) as u32
        };

        let mdhd = build_mdhd(track.timescale, media_duration);
        let handler = if track.track_info.kind == TrackKind::Video {
            HandlerType::Video
        } else {
            HandlerType::Audio
        };
        let hdlr = build_hdlr(handler);

        // stbl
        let stsd = build_stsd(&track.codec_config)?;
        let stts = build_stts(&track.samples);
        let ctts = build_ctts(&track.samples);
        let stsc = build_stsc(track.samples.len() as u32);
        let stsz = build_stsz(&track.samples);
        let stco = build_stco(&track.samples);
        let stss = build_stss(&track.samples);

        let mut stbl_body = stsd;
        stbl_body.extend_from_slice(&stts);
        if let Some(ctts) = ctts {
            stbl_body.extend_from_slice(&ctts);
        }
        stbl_body.extend_from_slice(&stsc);
        stbl_body.extend_from_slice(&stsz);
        stbl_body.extend_from_slice(&stco);
        if let Some(stss) = stss {
            stbl_body.extend_from_slice(&stss);
        }
        let stbl = make_box(b"stbl", &stbl_body);

        // media info header
        let xmhd = if track.track_info.kind == TrackKind::Video {
            build_vmhd()
        } else {
            build_smhd()
        };
        let dinf = build_dinf();
        let mut minf_body = xmhd;
        minf_body.extend_from_slice(&dinf);
        minf_body.extend_from_slice(&stbl);
        let minf = make_box(b"minf", &minf_body);

        let mut mdia_body = mdhd;
        mdia_body.extend_from_slice(&hdlr);
        mdia_body.extend_from_slice(&minf);
        let mdia = make_box(b"mdia", &mdia_body);

        let mut trak_body = tkhd;
        trak_body.extend_from_slice(&mdia);
        Ok(make_box(b"trak", &trak_body))
    }
}

impl<W: Write + Seek> Muxer for Mp4Muxer<W> {
    fn add_track(&mut self, info: &TrackInfo) -> Result<TrackIndex, MuxError> {
        // Infer minimal codec config from TrackInfo
        let config = match (&info.kind, &info.video, &info.audio) {
            (TrackKind::Video, Some(v), _) => {
                let is_h265 = matches!(info.codec, Codec::Video(VideoCodec::H265));
                if is_h265 {
                    CodecConfig::Hev1 {
                        width: v.width as u16,
                        height: v.height as u16,
                        hvcc: bytes::Bytes::new(),
                        color_space: v.color_space,
                    }
                } else {
                    CodecConfig::Avc1 {
                        width: v.width as u16,
                        height: v.height as u16,
                        avcc: bytes::Bytes::new(),
                        color_space: v.color_space,
                    }
                }
            }
            (TrackKind::Audio, _, Some(a)) => CodecConfig::Mp4a {
                sample_rate: a.sample_rate,
                channel_count: 2,
                esds: bytes::Bytes::new(),
            },
            _ => {
                return Err(MuxError::InvalidTrackConfig {
                    message: "track must have video or audio metadata".to_string(),
                })
            }
        };
        let timescale = match &info.audio {
            Some(a) => a.sample_rate,
            None => 90000,
        };
        self.add_track_with_config(info, config, timescale)
    }

    fn write_packet(&mut self, packet: &Packet) -> Result<(), MuxError> {
        self.write_packet_data(packet)
    }

    fn finalize(&mut self) -> Result<(), MuxError> {
        self.finalize_file()
    }
}

// ---------------------------------------------------------------------------
// Muxer-specific helpers (ftyp, sample table boxes)
// ---------------------------------------------------------------------------

fn build_ftyp() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"isom"); // major brand
    body.extend_from_slice(&0u32.to_be_bytes()); // minor version
    body.extend_from_slice(b"isom"); // compatible brands
    body.extend_from_slice(b"iso2");
    body.extend_from_slice(b"mp41");
    make_box(b"ftyp", &body)
}

fn build_stts(samples: &[MuxSample]) -> Vec<u8> {
    if samples.is_empty() {
        let mut body = vec![0, 0, 0, 0]; // version+flags
        body.extend_from_slice(&0u32.to_be_bytes());
        return make_full_box(b"stts", &body);
    }

    // Compute deltas and run-length encode
    let mut entries: Vec<(u32, u32)> = Vec::new(); // (count, delta)
    for i in 0..samples.len() {
        let delta = if i + 1 < samples.len() {
            (samples[i + 1].dts - samples[i].dts) as u32
        } else if !entries.is_empty() {
            // entries is guaranteed non-empty here because i > 0
            // implies at least one entry was pushed in a prior iteration
            entries.last().map(|e| e.1).unwrap_or(1)
        } else {
            1
        };

        if let Some(last) = entries.last_mut() {
            if last.1 == delta {
                last.0 += 1;
                continue;
            }
        }
        entries.push((1, delta));
    }

    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (count, delta) in &entries {
        body.extend_from_slice(&count.to_be_bytes());
        body.extend_from_slice(&delta.to_be_bytes());
    }
    make_full_box(b"stts", &body)
}

fn build_ctts(samples: &[MuxSample]) -> Option<Vec<u8>> {
    // Only needed if any sample has non-zero CTS offset
    if samples.iter().all(|s| s.cts_offset == 0) {
        return None;
    }

    let mut entries: Vec<(u32, i32)> = Vec::new();
    for sample in samples {
        if let Some(last) = entries.last_mut() {
            if last.1 == sample.cts_offset {
                last.0 += 1;
                continue;
            }
        }
        entries.push((1, sample.cts_offset));
    }

    // Use version 1 for signed offsets
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (count, offset) in &entries {
        body.extend_from_slice(&count.to_be_bytes());
        body.extend_from_slice(&(*offset as u32).to_be_bytes());
    }
    Some(make_full_box(b"ctts", &body))
}

fn build_stsc(sample_count: u32) -> Vec<u8> {
    // Simple: one chunk per sample
    let mut body = vec![0, 0, 0, 0]; // version+flags
    if sample_count > 0 {
        body.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        body.extend_from_slice(&1u32.to_be_bytes()); // first_chunk
        body.extend_from_slice(&1u32.to_be_bytes()); // samples_per_chunk
        body.extend_from_slice(&1u32.to_be_bytes()); // sample_description_index
    } else {
        body.extend_from_slice(&0u32.to_be_bytes());
    }
    make_full_box(b"stsc", &body)
}

fn build_stsz(samples: &[MuxSample]) -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&0u32.to_be_bytes()); // sample_size (0 = variable)
    body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for sample in samples {
        body.extend_from_slice(&sample.size.to_be_bytes());
    }
    make_full_box(b"stsz", &body)
}

fn build_stco(samples: &[MuxSample]) -> Vec<u8> {
    // Use co64 for safety (64-bit offsets)
    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for sample in samples {
        body.extend_from_slice(&sample.offset.to_be_bytes());
    }
    make_full_box(b"co64", &body)
}

fn build_stss(samples: &[MuxSample]) -> Option<Vec<u8>> {
    let keyframes: Vec<u32> = samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_sync)
        .map(|(i, _)| i as u32 + 1) // 1-based
        .collect();

    // If all samples are keyframes, stss is not needed
    if keyframes.len() == samples.len() {
        return None;
    }

    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&(keyframes.len() as u32).to_be_bytes());
    for kf in &keyframes {
        body.extend_from_slice(&kf.to_be_bytes());
    }
    Some(make_full_box(b"stss", &body))
}
