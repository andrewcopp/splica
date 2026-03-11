//! Fragmented MP4 (fMP4) muxer: writes compressed packets as a sequence of
//! (moof + mdat) fragments.
//!
//! Unlike `Mp4Muxer`, this does **not** require `Seek` — only `Write`.
//! This makes it suitable for streaming outputs and WASM targets where
//! seeking is unavailable.
//!
//! ## File structure
//!
//! ```text
//! ftyp
//! moov (mvhd + traks with empty stbl + mvex with trex per track)
//! moof (mfhd + traf per track (tfhd + tfdt + trun))
//! mdat
//! moof
//! mdat
//! ...
//! ```

use std::io::Write;

use splica_core::{MuxError, Muxer, Packet, TrackIndex, TrackInfo, TrackKind};

use crate::box_builders::{
    build_dinf, build_hdlr, build_mdhd, build_mvhd, build_smhd, build_stsd, build_tkhd, build_vmhd,
    io_err, make_box, make_full_box,
};
use crate::boxes::hdlr::HandlerType;
use crate::boxes::stsd::CodecConfig;

/// A sample buffered within a fragment before it is flushed.
struct FragmentSample {
    data: Vec<u8>,
    dts_ticks: i64,
    cts_offset: i32,
    is_keyframe: bool,
}

/// Per-track state for the fragmented muxer.
struct FMuxTrack {
    track_info: TrackInfo,
    codec_config: CodecConfig,
    timescale: u32,
    /// Samples buffered for the current fragment.
    pending_samples: Vec<FragmentSample>,
    /// Decode time of the next sample (in track timescale ticks).
    base_decode_time: u64,
}

/// Configuration for fragment flushing behavior.
#[derive(Debug, Clone)]
pub struct FragmentConfig {
    /// Maximum number of samples per fragment before auto-flushing.
    /// Defaults to 1 (one sample per fragment), which is safest for
    /// streaming but produces more overhead. Higher values reduce overhead.
    pub max_samples_per_fragment: u32,
}

impl Default for FragmentConfig {
    fn default() -> Self {
        Self {
            max_samples_per_fragment: 1,
        }
    }
}

/// A fragmented MP4 muxer that writes to any `Write` destination.
///
/// Does not require `Seek` — suitable for streaming, pipes, and WASM.
pub struct FragmentedMp4Muxer<W> {
    writer: W,
    tracks: Vec<FMuxTrack>,
    /// Whether the init segment (ftyp + moov) has been written.
    init_written: bool,
    /// Monotonically increasing fragment sequence number.
    sequence_number: u32,
    /// Fragment configuration.
    config: FragmentConfig,
}

impl<W: Write> FragmentedMp4Muxer<W> {
    /// Creates a new fragmented MP4 muxer with default configuration.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            tracks: Vec::new(),
            init_written: false,
            sequence_number: 1,
            config: FragmentConfig::default(),
        }
    }

    /// Creates a new fragmented MP4 muxer with the given configuration.
    pub fn with_config(writer: W, config: FragmentConfig) -> Self {
        Self {
            writer,
            tracks: Vec::new(),
            init_written: false,
            sequence_number: 1,
            config,
        }
    }

    /// Registers a track with full codec configuration.
    pub fn add_track_with_config(
        &mut self,
        info: &TrackInfo,
        codec_config: CodecConfig,
        timescale: u32,
    ) -> Result<TrackIndex, MuxError> {
        let index = TrackIndex(self.tracks.len() as u32);
        self.tracks.push(FMuxTrack {
            track_info: info.clone(),
            codec_config,
            timescale,
            pending_samples: Vec::new(),
            base_decode_time: 0,
        });
        Ok(index)
    }

    /// Writes the init segment (ftyp + moov with mvex) if not already written.
    fn ensure_init_written(&mut self) -> Result<(), MuxError> {
        if self.init_written {
            return Ok(());
        }

        let ftyp = build_fmp4_ftyp();
        self.writer.write_all(&ftyp).map_err(io_err)?;

        let moov = self.build_init_moov()?;
        self.writer.write_all(&moov).map_err(io_err)?;

        self.init_written = true;
        Ok(())
    }

    /// Builds the initialization moov box (empty stbl + mvex).
    fn build_init_moov(&self) -> Result<Vec<u8>, MuxError> {
        let movie_timescale = 1000u32;

        // mvhd with duration=0 (fragmented files don't know total duration upfront)
        let mvhd = build_mvhd(movie_timescale, 0);

        let mut trak_boxes = Vec::new();
        for (i, track) in self.tracks.iter().enumerate() {
            let trak = self.build_init_trak(track, i as u32 + 1)?;
            trak_boxes.extend_from_slice(&trak);
        }

        // mvex: one trex per track
        let mut mvex_body = Vec::new();
        for i in 0..self.tracks.len() {
            let trex = build_trex(i as u32 + 1);
            mvex_body.extend_from_slice(&trex);
        }
        let mvex = make_box(b"mvex", &mvex_body);

        let mut moov_body = mvhd;
        moov_body.extend_from_slice(&trak_boxes);
        moov_body.extend_from_slice(&mvex);

        Ok(make_box(b"moov", &moov_body))
    }

    /// Builds a trak box for the init segment (empty stbl).
    fn build_init_trak(&self, track: &FMuxTrack, track_id: u32) -> Result<Vec<u8>, MuxError> {
        let (width, height) = match &track.track_info.video {
            Some(v) => (v.width, v.height),
            None => (0, 0),
        };

        let tkhd = build_tkhd(track_id, 0, width, height);

        let mdhd = build_mdhd(track.timescale, 0);
        let handler = if track.track_info.kind == TrackKind::Video {
            HandlerType::Video
        } else {
            HandlerType::Audio
        };
        let hdlr = build_hdlr(handler);

        // stsd with codec config
        let stsd = build_stsd(&track.codec_config)?;

        // Empty sample table boxes (required by spec but empty for fMP4)
        let stts = build_empty_stts();
        let stsc = build_empty_stsc();
        let stsz = build_empty_stsz();
        let stco = build_empty_stco();

        let mut stbl_body = stsd;
        stbl_body.extend_from_slice(&stts);
        stbl_body.extend_from_slice(&stsc);
        stbl_body.extend_from_slice(&stsz);
        stbl_body.extend_from_slice(&stco);
        let stbl = make_box(b"stbl", &stbl_body);

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

    /// Writes a packet, buffering it and flushing fragments as needed.
    pub fn write_packet_data(&mut self, packet: &Packet) -> Result<(), MuxError> {
        self.ensure_init_written()?;

        let track_idx = packet.track_index.0 as usize;
        if track_idx >= self.tracks.len() {
            return Err(MuxError::InvalidTrackConfig {
                message: format!("track index {} out of range", track_idx),
            });
        }

        let dts_ticks = packet.dts.ticks();
        let cts_offset = (packet.pts.ticks() - packet.dts.ticks()) as i32;

        self.tracks[track_idx].pending_samples.push(FragmentSample {
            data: packet.data.to_vec(),
            dts_ticks,
            cts_offset,
            is_keyframe: packet.is_keyframe,
        });

        // Auto-flush if we've accumulated enough samples
        let pending_count = self.tracks[track_idx].pending_samples.len() as u32;
        if pending_count >= self.config.max_samples_per_fragment {
            self.flush_fragment()?;
        }

        Ok(())
    }

    /// Flushes all buffered samples across all tracks as a single moof+mdat pair.
    pub fn flush_fragment(&mut self) -> Result<(), MuxError> {
        // Check if any track has pending samples
        let has_samples = self.tracks.iter().any(|t| !t.pending_samples.is_empty());
        if !has_samples {
            return Ok(());
        }

        // Collect all sample data for mdat
        let mut mdat_body = Vec::new();
        let mut track_data_offsets: Vec<usize> = Vec::new();

        for track in &self.tracks {
            track_data_offsets.push(mdat_body.len());
            for sample in &track.pending_samples {
                mdat_body.extend_from_slice(&sample.data);
            }
        }

        // Build moof with placeholder data_offset, then measure and patch
        let moof_with_placeholder = self.build_moof(&track_data_offsets, 0)?;
        let moof_size = moof_with_placeholder.len() as u32;
        let mdat_header_size = 8u32; // standard box header

        // Rebuild moof with correct data_offset (relative to moof start)
        let moof = self.build_moof(&track_data_offsets, moof_size + mdat_header_size)?;

        // Write moof
        self.writer.write_all(&moof).map_err(io_err)?;

        // Write mdat
        let mdat = make_box(b"mdat", &mdat_body);
        self.writer.write_all(&mdat).map_err(io_err)?;

        // Update base_decode_time for each track and clear pending samples
        for track in &mut self.tracks {
            if !track.pending_samples.is_empty() {
                let last = track.pending_samples.last().unwrap();
                let last_dts = last.dts_ticks;
                // Estimate next base_decode_time from last sample's DTS + one delta
                let delta = if track.pending_samples.len() > 1 {
                    let second_last = &track.pending_samples[track.pending_samples.len() - 2];
                    (last_dts - second_last.dts_ticks).unsigned_abs()
                } else if last_dts > track.base_decode_time as i64 {
                    last_dts as u64 - track.base_decode_time
                } else {
                    1
                };
                track.base_decode_time = last_dts as u64 + delta;
                track.pending_samples.clear();
            }
        }

        self.sequence_number += 1;
        Ok(())
    }

    /// Builds a moof box for the current pending samples.
    ///
    /// `track_data_offsets` contains the byte offset of each track's data
    /// within the mdat body. `data_offset_base` is added to convert these
    /// into offsets relative to the moof start.
    fn build_moof(
        &self,
        track_data_offsets: &[usize],
        data_offset_base: u32,
    ) -> Result<Vec<u8>, MuxError> {
        let mfhd = build_mfhd(self.sequence_number);

        let mut traf_boxes = Vec::new();
        for (i, track) in self.tracks.iter().enumerate() {
            if track.pending_samples.is_empty() {
                continue;
            }
            let data_offset = data_offset_base + track_data_offsets[i] as u32;
            let traf = build_traf(i as u32 + 1, track, data_offset)?;
            traf_boxes.extend_from_slice(&traf);
        }

        let mut moof_body = mfhd;
        moof_body.extend_from_slice(&traf_boxes);
        Ok(make_box(b"moof", &moof_body))
    }

    /// Finalizes the fragmented MP4 by flushing any remaining samples.
    pub fn finalize_file(&mut self) -> Result<(), MuxError> {
        self.ensure_init_written()?;
        self.flush_fragment()?;
        self.writer.flush().map_err(io_err)?;
        Ok(())
    }
}

impl<W: Write> Muxer for FragmentedMp4Muxer<W> {
    fn add_track(&mut self, info: &TrackInfo) -> Result<TrackIndex, MuxError> {
        let config = match (&info.kind, &info.video, &info.audio) {
            (TrackKind::Video, Some(v), _) => CodecConfig::Avc1 {
                width: v.width as u16,
                height: v.height as u16,
                avcc: bytes::Bytes::new(),
                color_space: v.color_space,
            },
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
// fMP4-specific box building helpers
// ---------------------------------------------------------------------------

/// ftyp for fragmented MP4 — uses iso5/iso6/msdh brands.
fn build_fmp4_ftyp() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"iso5"); // major brand
    body.extend_from_slice(&1u32.to_be_bytes()); // minor version
    body.extend_from_slice(b"iso5"); // compatible brands
    body.extend_from_slice(b"iso6");
    body.extend_from_slice(b"msdh");
    body.extend_from_slice(b"msix");
    make_box(b"ftyp", &body)
}

// Empty sample table boxes for the init segment
fn build_empty_stts() -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0]; // version+flags
    body.extend_from_slice(&0u32.to_be_bytes()); // entry_count = 0
    make_full_box(b"stts", &body)
}

fn build_empty_stsc() -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0];
    body.extend_from_slice(&0u32.to_be_bytes());
    make_full_box(b"stsc", &body)
}

fn build_empty_stsz() -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0];
    body.extend_from_slice(&0u32.to_be_bytes()); // default_sample_size
    body.extend_from_slice(&0u32.to_be_bytes()); // sample_count
    make_full_box(b"stsz", &body)
}

fn build_empty_stco() -> Vec<u8> {
    let mut body = vec![0, 0, 0, 0];
    body.extend_from_slice(&0u32.to_be_bytes()); // entry_count
    make_full_box(b"stco", &body)
}

/// trex (Track Extends) — default values for track fragments.
fn build_trex(track_id: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&track_id.to_be_bytes());
    body.extend_from_slice(&1u32.to_be_bytes()); // default_sample_description_index
    body.extend_from_slice(&0u32.to_be_bytes()); // default_sample_duration
    body.extend_from_slice(&0u32.to_be_bytes()); // default_sample_size
    body.extend_from_slice(&0u32.to_be_bytes()); // default_sample_flags
    make_full_box(b"trex", &body)
}

/// mfhd (Movie Fragment Header).
fn build_mfhd(sequence_number: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0, 0, 0, 0]); // version=0, flags=0
    body.extend_from_slice(&sequence_number.to_be_bytes());
    make_full_box(b"mfhd", &body)
}

/// Build a traf box for one track's fragment.
fn build_traf(track_id: u32, track: &FMuxTrack, data_offset: u32) -> Result<Vec<u8>, MuxError> {
    let tfhd = build_tfhd(track_id);
    let tfdt = build_tfdt(track.base_decode_time);
    let trun = build_trun(&track.pending_samples, data_offset);

    let mut traf_body = tfhd;
    traf_body.extend_from_slice(&tfdt);
    traf_body.extend_from_slice(&trun);
    Ok(make_box(b"traf", &traf_body))
}

/// tfhd (Track Fragment Header).
///
/// Uses flag 0x020000 (default-base-is-moof) so data_offset in trun
/// is relative to the moof box start.
fn build_tfhd(track_id: u32) -> Vec<u8> {
    // version=0, flags=0x020000 (default-base-is-moof)
    let mut body = vec![0u8, 0x02, 0x00, 0x00];
    body.extend_from_slice(&track_id.to_be_bytes());
    make_full_box(b"tfhd", &body)
}

/// tfdt (Track Fragment Decode Time) — version 1 for 64-bit time.
fn build_tfdt(base_media_decode_time: u64) -> Vec<u8> {
    let mut body = Vec::new();
    // version=1, flags=0
    body.extend_from_slice(&[1, 0, 0, 0]);
    body.extend_from_slice(&base_media_decode_time.to_be_bytes());
    make_full_box(b"tfdt", &body)
}

/// trun (Track Fragment Run).
///
/// Flags: 0x000001 (data_offset_present)
///      | 0x000100 (sample_duration_present)
///      | 0x000200 (sample_size_present)
///      | 0x000400 (sample_flags_present)
///      | 0x000800 (sample_composition_time_offsets_present)
fn build_trun(samples: &[FragmentSample], data_offset: u32) -> Vec<u8> {
    // Determine if we need composition time offsets
    let has_cts = samples.iter().any(|s| s.cts_offset != 0);

    let mut flags: u32 = 0x000001 // data_offset_present
        | 0x000100  // sample_duration_present
        | 0x000200  // sample_size_present
        | 0x000400; // sample_flags_present
    if has_cts {
        flags |= 0x000800; // sample_composition_time_offsets_present
    }

    let mut body = Vec::new();
    // version=0 (version=1 needed for signed CTS offsets, but we use 0 for compatibility)
    let version: u8 = if has_cts { 1 } else { 0 };
    body.push(version);
    body.push((flags >> 16) as u8);
    body.push((flags >> 8) as u8);
    body.push(flags as u8);

    // sample_count
    body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    // data_offset (signed i32)
    body.extend_from_slice(&(data_offset as i32).to_be_bytes());

    // Per-sample entries
    for (i, sample) in samples.iter().enumerate() {
        // sample_duration: delta to next sample, or repeat last
        let duration = if i + 1 < samples.len() {
            (samples[i + 1].dts_ticks - sample.dts_ticks) as u32
        } else if i > 0 {
            (sample.dts_ticks - samples[i - 1].dts_ticks) as u32
        } else {
            1 // single sample — use 1 as fallback duration
        };
        body.extend_from_slice(&duration.to_be_bytes());

        // sample_size
        body.extend_from_slice(&(sample.data.len() as u32).to_be_bytes());

        // sample_flags
        let flags = if sample.is_keyframe {
            0x02000000u32 // sample_depends_on=2 (does not depend on others)
        } else {
            0x01010000u32 // sample_depends_on=1 (depends on others), sample_is_non_sync=1
        };
        body.extend_from_slice(&flags.to_be_bytes());

        // sample_composition_time_offset (if present)
        if has_cts {
            body.extend_from_slice(&(sample.cts_offset).to_be_bytes());
        }
    }

    make_full_box(b"trun", &body)
}
