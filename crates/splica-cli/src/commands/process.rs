use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use miette::{Context, IntoDiagnostic, Result};

use splica_codec::{
    AacDecoder, AacEncoderBuilder, Av1Decoder, Av1EncoderBuilder, H264Decoder, H264EncoderBuilder,
    H265Decoder, H265EncoderBuilder, OpusDecoder, OpusEncoderBuilder,
};
use splica_core::{
    AudioCodec, Codec, ContainerFormat, Demuxer, Muxer, TrackIndex, TrackKind, VideoCodec,
};
use splica_filter::{AspectMode, CropFilter, ScaleFilter, VolumeFilter};
use splica_mkv::MkvMuxer;
use splica_mp4::boxes::stsd::CodecConfig;
use splica_mp4::{Mp4Demuxer, Mp4Muxer};
use splica_pipeline::{PipelineBuilder, PipelineEventKind};
use splica_webm::{WebmDemuxer, WebmMuxer};

use super::{
    classify_error, create_muxer, detect_format, open_demuxer, output_container,
    validate_output_format, AspectModeArg, AudioMode, CompleteEvent, DetectedFormat, EncodePreset,
    ErrorResult, OutputFormat, ProgressEvent, TranscodeAudioInfo, VideoCodecArg,
};

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parses a bitrate string like "2M", "1500k", or "1000000" into bits per second.
fn parse_bitrate(s: &str) -> Result<u32> {
    let s = s.trim();
    if let Some(prefix) = s.strip_suffix('M') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000_000.0) as u32)
    } else if let Some(prefix) = s.strip_suffix('m') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000_000.0) as u32)
    } else if let Some(prefix) = s.strip_suffix('k') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000.0) as u32)
    } else if let Some(prefix) = s.strip_suffix('K') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000.0) as u32)
    } else {
        s.parse::<u32>().into_diagnostic().wrap_err_with(|| {
            format!("invalid bitrate: '{s}' — use e.g. '2M', '1500k', or raw bps")
        })
    }
}

/// Parses a "WxH" resize string into (width, height).
fn parse_resize(s: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = s.split('x').collect();
    if parts.len() != 2 {
        return Err(miette::miette!(
            "invalid resize format: '{s}' — use WxH (e.g., '1280x720')"
        ));
    }
    let w: u32 = parts[0]
        .parse()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid width in resize: '{s}'"))?;
    let h: u32 = parts[1]
        .parse()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid height in resize: '{s}'"))?;
    if w == 0 || h == 0 {
        return Err(miette::miette!("resize dimensions must be non-zero: '{s}'"));
    }
    Ok((w, h))
}

/// Parses a "WxH+X+Y" crop geometry string into (x, y, width, height).
fn parse_crop(s: &str) -> Result<(u32, u32, u32, u32)> {
    // Expected format: WxH+X+Y (e.g., "1080x1080+420+0")
    let parts: Vec<&str> = s.splitn(2, 'x').collect();
    if parts.len() != 2 {
        return Err(miette::miette!(
            "invalid crop format: '{s}' — use WxH+X+Y (e.g., '1080x1080+420+0')"
        ));
    }
    let w: u32 = parts[0]
        .parse()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid width in crop: '{s}'"))?;

    let rest = parts[1];
    let plus_parts: Vec<&str> = rest.splitn(3, '+').collect();
    if plus_parts.len() != 3 {
        return Err(miette::miette!(
            "invalid crop format: '{s}' — use WxH+X+Y (e.g., '1080x1080+420+0')"
        ));
    }
    let h: u32 = plus_parts[0]
        .parse()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid height in crop: '{s}'"))?;
    let x: u32 = plus_parts[1]
        .parse()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid X offset in crop: '{s}'"))?;
    let y: u32 = plus_parts[2]
        .parse()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid Y offset in crop: '{s}'"))?;

    if w == 0 || h == 0 {
        return Err(miette::miette!("crop dimensions must be non-zero: '{s}'"));
    }

    Ok((x, y, w, h))
}

fn parse_volume(s: &str) -> Result<VolumeFilter> {
    let s = s.trim();
    if let Some(db_str) = s.strip_suffix("dB").or_else(|| s.strip_suffix("db")) {
        let db: f32 = db_str
            .trim()
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid dB value in volume: '{s}'"))?;
        VolumeFilter::from_db(db)
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid volume: '{s}'"))
    } else {
        let gain: f32 = s.parse().into_diagnostic().wrap_err_with(|| {
            format!("invalid volume: '{s}' — use a number (e.g., '0.5') or dB value (e.g., '-6dB')")
        })?;
        VolumeFilter::new(gain)
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid volume: '{s}'"))
    }
}

// ---------------------------------------------------------------------------
// Process types
// ---------------------------------------------------------------------------

pub(crate) struct ProcessArgs<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub bitrate: Option<&'a str>,
    pub crf: Option<u8>,
    pub preset: Option<&'a EncodePreset>,
    pub max_fps: Option<f32>,
    pub resize: Option<&'a str>,
    pub aspect_mode_arg: &'a AspectModeArg,
    pub crop: Option<&'a str>,
    pub volume: Option<&'a str>,
    pub codec: Option<&'a VideoCodecArg>,
}

struct TranscodeOutput {
    packets_read: u64,
    frames_decoded: u64,
    frames_encoded: u64,
    packets_written: u64,
    audio_tracks: Vec<TranscodeAudioInfo>,
    mux_ok: bool,
    output_codec: Option<String>,
    output_duration_secs: Option<f64>,
    output_bitrate_kbps: Option<u64>,
}

/// Audio codec config extracted from the demuxer for audio tracks.
#[derive(Debug, Clone)]
struct AudioCodecConfig {
    track_index: TrackIndex,
    codec: splica_core::AudioCodec,
    /// Raw codec-specific config (e.g., esds for AAC).
    config_data: Option<Vec<u8>>,
    sample_rate: u32,
    channel_layout: Option<splica_core::media::ChannelLayout>,
}

/// Video codec configuration extracted from the container for re-encoding.
enum VideoTrackCodec {
    H264,
    H265,
    Av1,
}

type DemuxerWithConfigs = (
    Box<dyn Demuxer>,
    Vec<(
        TrackIndex,
        VideoTrackCodec,
        Vec<u8>,
        Option<splica_core::ColorSpace>,
        u32, // width
        u32, // height
    )>,
    Vec<AudioCodecConfig>,
);

/// Returns true if the video codec can be stream-copied into the target container.
fn is_video_codec_compatible(codec: &Codec, container: ContainerFormat) -> bool {
    match codec {
        Codec::Video(vc) => match container {
            ContainerFormat::WebM => {
                // WebM supports VP8, VP9, AV1
                matches!(vc, VideoCodec::Av1)
                    || matches!(vc, VideoCodec::Other(s) if s == "VP8" || s == "VP9")
            }
            ContainerFormat::Mp4 => {
                // MP4 supports H.264, H.265, AV1
                matches!(vc, VideoCodec::H264 | VideoCodec::H265 | VideoCodec::Av1)
            }
            ContainerFormat::Mkv => {
                // MKV supports all codecs splica handles
                matches!(vc, VideoCodec::H264 | VideoCodec::H265 | VideoCodec::Av1)
                    || matches!(vc, VideoCodec::Other(s) if s == "VP8" || s == "VP9")
            }
        },
        Codec::Audio(_) => true, // audio compatibility is handled separately
    }
}

/// QC metadata extracted from the output file after muxing.
struct OutputQc {
    codec: Option<String>,
    duration_secs: Option<f64>,
    bitrate_kbps: Option<u64>,
}

/// Probes the output file to extract QC metadata for the complete event.
fn probe_output_qc(path: &Path) -> OutputQc {
    let demuxer = match open_demuxer(path) {
        Ok(d) => d,
        Err(_) => {
            return OutputQc {
                codec: None,
                duration_secs: None,
                bitrate_kbps: None,
            }
        }
    };

    let video_track = demuxer.tracks().iter().find(|t| t.kind == TrackKind::Video);

    let codec = video_track.map(|t| t.codec.to_string());
    let duration_secs = demuxer
        .tracks()
        .iter()
        .filter_map(|t| t.duration.map(|d| d.as_seconds_f64()))
        .reduce(f64::max);

    let bitrate_kbps = duration_secs.and_then(|dur| {
        if dur <= 0.0 {
            return None;
        }
        let file_size = std::fs::metadata(path).ok()?.len();
        Some((file_size as f64 * 8.0 / dur / 1000.0) as u64)
    });

    OutputQc {
        codec,
        duration_secs,
        bitrate_kbps,
    }
}

// ---------------------------------------------------------------------------
// Stream copy
// ---------------------------------------------------------------------------

/// Stream copy path: copies packets without re-encoding.
/// Returns TranscodeOutput with zero encode/decode counts.
fn stream_copy(args: &ProcessArgs<'_>, json_mode: bool) -> Result<TranscodeOutput> {
    let mut demuxer = open_demuxer(args.input)?;

    let out_file = File::create(args.output)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not create output file '{}'", args.output.display()))?;

    let mut muxer: Box<dyn Muxer> = match output_container(args.output) {
        Some(ContainerFormat::WebM) => Box::new(WebmMuxer::new(BufWriter::new(out_file))),
        Some(ContainerFormat::Mkv) => Box::new(MkvMuxer::new(BufWriter::new(out_file))),
        _ => Box::new(Mp4Muxer::new(BufWriter::new(out_file))),
    };

    let tracks = demuxer.tracks().to_vec();
    let track_count = tracks.len();

    // Collect audio track info for JSON output
    let audio_tracks: Vec<TranscodeAudioInfo> = tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .map(|t| {
            let codec = t.codec.to_string();
            let sample_rate = t.audio.as_ref().map(|a| a.sample_rate).unwrap_or(0);
            let channels = t
                .audio
                .as_ref()
                .and_then(|a| a.channel_layout.map(|cl| cl.channel_count()));
            TranscodeAudioInfo {
                codec,
                sample_rate,
                channels,
                mode: AudioMode::PassThrough,
            }
        })
        .collect();

    for i in 0..track_count {
        let info = demuxer.tracks()[i].clone();
        muxer
            .add_track(&info)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to add track {i}"))?;
    }

    if !json_mode {
        eprintln!(
            "Processing {} → {} (stream copy)",
            args.input.display(),
            args.output.display()
        );
    }

    let mut packet_count: u64 = 0;
    while let Some(packet) = demuxer
        .read_packet()
        .into_diagnostic()
        .wrap_err("failed to read packet")?
    {
        muxer
            .write_packet(&packet)
            .into_diagnostic()
            .wrap_err("failed to write packet")?;
        packet_count += 1;
    }

    let mux_result = muxer.finalize();
    let mux_ok = mux_result.is_ok();

    if !json_mode {
        if mux_ok {
            eprintln!("  Done. Copied {packet_count} packets across {track_count} tracks.");
        }
        mux_result
            .into_diagnostic()
            .wrap_err("failed to finalize output")?;
    }

    let qc = if json_mode {
        probe_output_qc(args.output)
    } else {
        OutputQc {
            codec: None,
            duration_secs: None,
            bitrate_kbps: None,
        }
    };

    Ok(TranscodeOutput {
        packets_read: packet_count,
        frames_decoded: 0,
        frames_encoded: 0,
        packets_written: packet_count,
        audio_tracks,
        mux_ok,
        output_codec: qc.codec,
        output_duration_secs: qc.duration_secs,
        output_bitrate_kbps: qc.bitrate_kbps,
    })
}

// ---------------------------------------------------------------------------
// Process command
// ---------------------------------------------------------------------------

pub(crate) fn process(args: &ProcessArgs<'_>, format: &OutputFormat) -> Result<()> {
    let json_mode = matches!(format, OutputFormat::Json);

    // In JSON mode, catch errors and emit structured JSON instead of miette output
    let result = process_inner(args, json_mode);

    if json_mode {
        match result {
            Ok(out) => {
                let complete = CompleteEvent {
                    event_type: "complete",
                    input: args.input.display().to_string(),
                    output: args.output.display().to_string(),
                    packets_read: out.packets_read,
                    frames_decoded: out.frames_decoded,
                    frames_encoded: out.frames_encoded,
                    packets_written: out.packets_written,
                    audio_tracks: out.audio_tracks,
                    mux_ok: out.mux_ok,
                    output_codec: out.output_codec,
                    output_duration_secs: out.output_duration_secs,
                    output_bitrate_kbps: out.output_bitrate_kbps,
                };
                println!("{}", serde_json::to_string(&complete).unwrap());
                Ok(())
            }
            Err(e) => {
                let (error_kind, code) = classify_error(&e);
                let error_json = ErrorResult {
                    event_type: "error",
                    error_kind: error_kind.to_string(),
                    message: format!("{e}"),
                };
                println!("{}", serde_json::to_string(&error_json).unwrap());
                std::process::exit(code);
            }
        }
    } else {
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

fn process_inner(args: &ProcessArgs<'_>, json_mode: bool) -> Result<TranscodeOutput> {
    validate_output_format(args.output)?;

    // Determine if user explicitly requested re-encoding via any encoding option
    let user_requested_reencode = args.bitrate.is_some()
        || args.crf.is_some()
        || args.preset.is_some()
        || args.max_fps.is_some()
        || args.resize.is_some()
        || args.crop.is_some()
        || args.volume.is_some()
        || args.codec.is_some();

    let effective_preset = args.preset.unwrap_or(&EncodePreset::Medium);

    // Resolve quality target: --crf and --bitrate are mutually exclusive (enforced by clap).
    // When neither is given, fall back to preset-based bitrate defaults.
    let quality_target: splica_core::QualityTarget = if let Some(crf) = args.crf {
        splica_core::QualityTarget::crf(crf)
            .ok_or_else(|| miette::miette!("CRF value {crf} is out of range — must be 0–51"))?
    } else if let Some(s) = args.bitrate {
        splica_core::QualityTarget::Bitrate(parse_bitrate(s)?)
    } else {
        let bps = match effective_preset {
            EncodePreset::Fast => 500_000,     // 500 kbps
            EncodePreset::Medium => 1_000_000, // 1 Mbps
            EncodePreset::Slow => 2_000_000,   // 2 Mbps
        };
        splica_core::QualityTarget::Bitrate(bps)
    };

    let frame_rate_hint = args.max_fps.unwrap_or(match effective_preset {
        EncodePreset::Fast => 30.0,
        EncodePreset::Medium => 30.0,
        EncodePreset::Slow => 60.0,
    });

    // Try MP4 first (for codec config access), fall back to generic demuxer
    let mut file = File::open(args.input)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open file '{}'", args.input.display()))?;
    let format = detect_format(&mut file)?;

    let (demuxer, video_track_configs, audio_track_configs): DemuxerWithConfigs = match format {
        DetectedFormat::Mp4 => {
            let mp4 = Mp4Demuxer::open(file)
                .into_diagnostic()
                .wrap_err("failed to parse MP4 container")?;
            let tracks = mp4.tracks().to_vec();
            let mut video_configs = Vec::new();
            let mut audio_configs = Vec::new();
            for track in &tracks {
                if track.kind == TrackKind::Video {
                    match &track.codec {
                        Codec::Video(VideoCodec::H264) => {
                            if let Some(CodecConfig::Avc1 {
                                avcc,
                                color_space,
                                width,
                                height,
                            }) = mp4.codec_config(track.index)
                            {
                                video_configs.push((
                                    track.index,
                                    VideoTrackCodec::H264,
                                    avcc.to_vec(),
                                    *color_space,
                                    *width as u32,
                                    *height as u32,
                                ));
                            }
                        }
                        Codec::Video(VideoCodec::H265) => {
                            if let Some(CodecConfig::Hev1 {
                                hvcc,
                                color_space,
                                width,
                                height,
                            }) = mp4.codec_config(track.index)
                            {
                                video_configs.push((
                                    track.index,
                                    VideoTrackCodec::H265,
                                    hvcc.to_vec(),
                                    *color_space,
                                    *width as u32,
                                    *height as u32,
                                ));
                            }
                        }
                        Codec::Video(VideoCodec::Av1) => {
                            if let Some(CodecConfig::Av1 {
                                av1c,
                                color_space,
                                width,
                                height,
                            }) = mp4.codec_config(track.index)
                            {
                                video_configs.push((
                                    track.index,
                                    VideoTrackCodec::Av1,
                                    av1c.to_vec(),
                                    *color_space,
                                    *width as u32,
                                    *height as u32,
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                if track.kind == TrackKind::Audio {
                    if let Codec::Audio(ref audio_codec) = track.codec {
                        let config_data = if let Some(CodecConfig::Mp4a { esds, .. }) =
                            mp4.codec_config(track.index)
                        {
                            Some(esds.to_vec())
                        } else {
                            None
                        };
                        audio_configs.push(AudioCodecConfig {
                            track_index: track.index,
                            codec: audio_codec.clone(),
                            config_data,
                            sample_rate: track
                                .audio
                                .as_ref()
                                .map(|a| a.sample_rate)
                                .unwrap_or(44100),
                            channel_layout: track.audio.as_ref().and_then(|a| a.channel_layout),
                        });
                    }
                }
            }
            (Box::new(mp4), video_configs, audio_configs)
        }
        DetectedFormat::WebM => {
            let webm = WebmDemuxer::open(BufReader::new(file))
                .into_diagnostic()
                .wrap_err("failed to parse WebM container")?;
            let tracks = webm.tracks().to_vec();
            let mut audio_configs = Vec::new();
            for track in &tracks {
                if track.kind == TrackKind::Audio {
                    if let Codec::Audio(ref audio_codec) = track.codec {
                        audio_configs.push(AudioCodecConfig {
                            track_index: track.index,
                            codec: audio_codec.clone(),
                            config_data: None,
                            sample_rate: track
                                .audio
                                .as_ref()
                                .map(|a| a.sample_rate)
                                .unwrap_or(48000),
                            channel_layout: track.audio.as_ref().and_then(|a| a.channel_layout),
                        });
                    }
                }
            }
            // WebM doesn't expose MP4-style codec config; H.264 in WebM is unsupported
            (Box::new(webm), Vec::new(), audio_configs)
        }
    };

    // Determine target audio codec based on output container
    let out_container = output_container(args.output).unwrap_or(ContainerFormat::Mp4);
    let target_audio_codec = match out_container {
        ContainerFormat::WebM | ContainerFormat::Mkv => splica_core::AudioCodec::Opus,
        ContainerFormat::Mp4 => splica_core::AudioCodec::Aac,
    };

    // Determine which audio tracks need transcoding vs pass-through.
    // Volume adjustment requires re-encoding even if the codec matches.
    let volume_requested = args.volume.is_some();
    let audio_needs_transcode: Vec<bool> = audio_track_configs
        .iter()
        .map(|ac| ac.codec != target_audio_codec || volume_requested)
        .collect();

    // Collect audio track metadata for JSON output
    let audio_tracks: Vec<TranscodeAudioInfo> = audio_track_configs
        .iter()
        .zip(audio_needs_transcode.iter())
        .map(|(ac, &needs_transcode)| {
            let codec = match &ac.codec {
                splica_core::AudioCodec::Aac => "AAC".to_string(),
                splica_core::AudioCodec::Opus => "Opus".to_string(),
                splica_core::AudioCodec::Other(s) => s.clone(),
            };
            TranscodeAudioInfo {
                codec,
                sample_rate: ac.sample_rate,
                channels: ac.channel_layout.map(|cl| cl.channel_count()),
                mode: if needs_transcode {
                    AudioMode::ReEncoded
                } else {
                    AudioMode::PassThrough
                },
            }
        })
        .collect();

    // Check if any video codec is incompatible with the output container
    let video_needs_reencode = demuxer
        .tracks()
        .iter()
        .filter(|t| t.kind == TrackKind::Video)
        .any(|t| !is_video_codec_compatible(&t.codec, out_container));

    let any_audio_needs_transcode = audio_needs_transcode.iter().any(|&b| b);

    // Decide: stream copy vs re-encode
    // Stream copy when: no user encoding flags, all codecs compatible
    let use_stream_copy =
        !user_requested_reencode && !video_needs_reencode && !any_audio_needs_transcode;

    if use_stream_copy {
        return stream_copy(args, json_mode);
    }

    // Re-encode path: we need video tracks with supported codec config to decode+encode
    if video_track_configs.is_empty() {
        return Err(miette::miette!(
            "no supported video tracks found in '{}' — re-encoding supports H.264, H.265, and AV1",
            args.input.display()
        ));
    }

    let muxer = create_muxer(args.output)?;

    // Shared counters for JSON output
    let counter_packets_read = Arc::new(AtomicU64::new(0));
    let counter_frames_decoded = Arc::new(AtomicU64::new(0));
    let counter_frames_encoded = Arc::new(AtomicU64::new(0));
    let counter_packets_written = Arc::new(AtomicU64::new(0));

    let pr = Arc::clone(&counter_packets_read);
    let fd = Arc::clone(&counter_frames_decoded);
    let fe = Arc::clone(&counter_frames_encoded);
    let pw = Arc::clone(&counter_packets_written);

    // Build pipeline: video tracks get decoder+encoder, audio tracks copy through
    let mut builder = PipelineBuilder::new()
        .with_event_handler(move |event| match event.kind {
            PipelineEventKind::PacketsRead { count } => {
                pr.store(count, Ordering::Relaxed);
                if json_mode {
                    if count % 100 == 0 {
                        let progress = ProgressEvent {
                            event_type: "progress",
                            packets_read: count,
                            frames_decoded: fd.load(Ordering::Relaxed),
                            frames_encoded: fe.load(Ordering::Relaxed),
                            packets_written: pw.load(Ordering::Relaxed),
                        };
                        // NDJSON: one compact JSON object per line
                        println!("{}", serde_json::to_string(&progress).unwrap());
                    }
                } else if count % 100 == 0 {
                    eprint!("\r  Packets read: {count}");
                }
            }
            PipelineEventKind::FramesDecoded { count } => {
                fd.store(count, Ordering::Relaxed);
                if !json_mode && count % 100 == 0 {
                    eprint!("  Decoded: {count}");
                }
            }
            PipelineEventKind::FramesEncoded { count } => {
                fe.store(count, Ordering::Relaxed);
                if !json_mode && count % 100 == 0 {
                    eprint!("  Encoded: {count}");
                }
            }
            PipelineEventKind::PacketsWritten { count } => {
                pw.store(count, Ordering::Relaxed);
            }
            _ => {}
        })
        .with_demuxer(demuxer)
        .with_muxer(muxer);

    for (track_idx, video_codec, config_data, color_space, src_width, src_height) in
        &video_track_configs
    {
        match video_codec {
            VideoTrackCodec::H264 => {
                let decoder = H264Decoder::new(config_data)
                    .into_diagnostic()
                    .wrap_err("failed to create H.264 decoder")?;
                builder = builder.with_decoder(*track_idx, decoder);
            }
            VideoTrackCodec::H265 => {
                let decoder = H265Decoder::new(config_data)
                    .into_diagnostic()
                    .wrap_err("failed to create H.265 decoder")?;
                builder = builder.with_decoder(*track_idx, decoder);
            }
            VideoTrackCodec::Av1 => {
                let decoder = Av1Decoder::new(config_data)
                    .into_diagnostic()
                    .wrap_err("failed to create AV1 decoder")?;
                builder = builder.with_decoder(*track_idx, decoder);
            }
        }

        // Select encoder based on output container and --codec flag
        let use_av1 = matches!(out_container, ContainerFormat::WebM);
        let use_h265 = matches!(args.codec, Some(VideoCodecArg::H265));

        // Determine encode dimensions (resize overrides source)
        let (enc_w, enc_h) = if let Some(resize_str) = args.resize {
            parse_resize(resize_str)?
        } else {
            (*src_width, *src_height)
        };

        if use_av1 {
            let mut enc_builder = Av1EncoderBuilder::new()
                .quality(quality_target)
                .track_index(*track_idx)
                .dimensions(enc_w, enc_h)
                .speed(6)
                .max_frame_rate(frame_rate_hint);

            if let Some(cs) = color_space {
                enc_builder = enc_builder.color_space(*cs);
            }

            let encoder = enc_builder
                .build()
                .into_diagnostic()
                .wrap_err("failed to create AV1 encoder")?;

            builder = builder.with_encoder(*track_idx, encoder);
        } else if use_h265 {
            let mut enc_builder = H265EncoderBuilder::new()
                .quality(quality_target)
                .track_index(*track_idx)
                .dimensions(enc_w, enc_h)
                .max_frame_rate(frame_rate_hint);

            if let Some(cs) = color_space {
                enc_builder = enc_builder.color_space(*cs);
            }

            let encoder = enc_builder
                .build()
                .into_diagnostic()
                .wrap_err("failed to create H.265 encoder")?;

            builder = builder
                .with_encoder(*track_idx, encoder)
                .with_output_codec(*track_idx, Codec::Video(VideoCodec::H265));
        } else {
            let mut enc_builder = H264EncoderBuilder::new()
                .quality(quality_target)
                .max_frame_rate(frame_rate_hint)
                .track_index(*track_idx);

            if let Some(cs) = color_space {
                enc_builder = enc_builder.color_space(*cs);
            }

            let encoder = enc_builder
                .build()
                .into_diagnostic()
                .wrap_err("failed to create H.264 encoder")?;

            builder = builder.with_encoder(*track_idx, encoder);
        }

        // Add scale filter if --resize was specified
        if let Some(resize_str) = args.resize {
            let (w, h) = parse_resize(resize_str)?;
            let aspect_mode = match args.aspect_mode_arg {
                AspectModeArg::Stretch => AspectMode::Stretch,
                AspectModeArg::Fit => AspectMode::Fit,
                AspectModeArg::Fill => AspectMode::Fill,
            };
            let scale_filter = ScaleFilter::new(w, h).with_aspect_mode(aspect_mode);
            builder = builder.with_filter(*track_idx, scale_filter);
        }

        // Add crop filter if --crop was specified (applied after scale)
        if let Some(crop_str) = args.crop {
            let (cx, cy, cw, ch) = parse_crop(crop_str)?;
            let crop_filter = CropFilter::new(cx, cy, cw, ch)
                .into_diagnostic()
                .wrap_err("invalid crop parameters")?;
            builder = builder.with_filter(*track_idx, crop_filter);
        }
    }

    // Wire audio decoder+encoder for tracks that need transcoding
    for (ac, &needs_transcode) in audio_track_configs.iter().zip(audio_needs_transcode.iter()) {
        if !needs_transcode {
            // Pass-through: no decoder/encoder needed, pipeline handles copy mode
            continue;
        }

        // Create audio decoder based on input codec
        match &ac.codec {
            AudioCodec::Aac => {
                let config_data = ac.config_data.as_ref().ok_or_else(|| {
                    miette::miette!(
                        "AAC audio track {} has no codec config (esds) — cannot decode",
                        ac.track_index.0
                    )
                })?;
                let decoder = AacDecoder::new(config_data)
                    .into_diagnostic()
                    .wrap_err("failed to create AAC decoder")?;
                builder = builder.with_audio_decoder(ac.track_index, decoder);
            }
            AudioCodec::Opus => {
                let channel_layout = ac
                    .channel_layout
                    .unwrap_or(splica_core::media::ChannelLayout::Stereo);
                let decoder = OpusDecoder::new(ac.sample_rate, channel_layout)
                    .into_diagnostic()
                    .wrap_err("failed to create Opus decoder")?;
                builder = builder.with_audio_decoder(ac.track_index, decoder);
            }
            AudioCodec::Other(name) => {
                return Err(miette::miette!(
                    "unsupported audio codec '{}' in track {} — cannot transcode",
                    name,
                    ac.track_index.0
                ));
            }
        }

        // Create audio encoder based on target codec
        let channel_layout = ac
            .channel_layout
            .unwrap_or(splica_core::media::ChannelLayout::Stereo);

        match &target_audio_codec {
            AudioCodec::Aac => {
                let encoder = AacEncoderBuilder::new()
                    .bitrate(128_000)
                    .sample_rate(ac.sample_rate)
                    .channel_layout(channel_layout)
                    .track_index(ac.track_index)
                    .build()
                    .into_diagnostic()
                    .wrap_err("failed to create AAC encoder")?;
                builder = builder.with_audio_encoder(ac.track_index, encoder);
            }
            AudioCodec::Opus => {
                let encoder = OpusEncoderBuilder::new()
                    .bitrate(128_000)
                    .sample_rate(ac.sample_rate)
                    .channel_layout(channel_layout)
                    .track_index(ac.track_index)
                    .build()
                    .into_diagnostic()
                    .wrap_err("failed to create Opus encoder")?;
                builder = builder.with_audio_encoder(ac.track_index, encoder);
            }
            AudioCodec::Other(name) => {
                return Err(miette::miette!(
                    "unsupported target audio codec '{}' — cannot encode",
                    name,
                ));
            }
        }
    }

    // Add volume filter to all transcoded audio tracks if --volume was specified
    if let Some(volume_str) = args.volume {
        let volume_filter = parse_volume(volume_str)?;
        for (ac, &needs_transcode) in audio_track_configs.iter().zip(audio_needs_transcode.iter()) {
            if needs_transcode {
                builder = builder.with_audio_filter(ac.track_index, volume_filter.clone());
            }
        }
    }

    let mut pipeline = builder
        .build()
        .into_diagnostic()
        .wrap_err("failed to build transcode pipeline")?;

    let preset_name = match effective_preset {
        EncodePreset::Fast => "fast",
        EncodePreset::Medium => "medium",
        EncodePreset::Slow => "slow",
    };
    let quality_desc = match quality_target {
        splica_core::QualityTarget::Crf(crf) => format!("CRF {crf}"),
        splica_core::QualityTarget::Bitrate(bps) => format!("{} kbps", bps / 1000),
    };
    if let Some(resize_str) = args.resize {
        eprintln!(
            "Processing {} → {} (re-encode, {quality_desc}, preset: {preset_name}, resize: {resize_str})",
            args.input.display(),
            args.output.display(),
        );
    } else {
        eprintln!(
            "Processing {} → {} (re-encode, {quality_desc}, preset: {preset_name})",
            args.input.display(),
            args.output.display(),
        );
    }

    pipeline
        .run()
        .into_diagnostic()
        .wrap_err("transcode failed")?;

    if !json_mode {
        eprintln!("\r  Done.                                        ");
    }

    let qc = if json_mode {
        probe_output_qc(args.output)
    } else {
        OutputQc {
            codec: None,
            duration_secs: None,
            bitrate_kbps: None,
        }
    };

    Ok(TranscodeOutput {
        packets_read: counter_packets_read.load(Ordering::Relaxed),
        frames_decoded: counter_frames_decoded.load(Ordering::Relaxed),
        frames_encoded: counter_frames_encoded.load(Ordering::Relaxed),
        packets_written: counter_packets_written.load(Ordering::Relaxed),
        audio_tracks,
        mux_ok: true,
        output_codec: qc.codec,
        output_duration_secs: qc.duration_secs,
        output_bitrate_kbps: qc.bitrate_kbps,
    })
}
