use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use miette::{Context, IntoDiagnostic, Result};

use splica_codec::{
    AacDecoder, AacEncoderBuilder, Av1Decoder, Av1EncoderBuilder, H264Decoder, H264EncoderBuilder,
    H265Decoder, H265EncoderBuilder, OpusDecoder, OpusEncoderBuilder,
};
use splica_core::{AudioCodec, Codec, ContainerFormat, TrackKind, VideoCodec};
use splica_filter::{AspectMode, CropFilter, ScaleFilter};
use splica_pipeline::{PipelineBuilder, PipelineEventKind};

use super::args::{
    open_demuxer_with_configs, parse_bitrate, parse_crop, parse_resize, parse_volume,
    DemuxerWithConfigs, ProcessArgs, VideoTrackCodec,
};
use super::TranscodeOutput;
use crate::commands::{
    create_muxer, open_demuxer, output_container, validate_output_format, AspectModeArg, AudioMode,
    EncodePreset, ProgressEvent, TranscodeAudioInfo, VideoCodecArg,
};

// ---------------------------------------------------------------------------
// Codec compatibility
// ---------------------------------------------------------------------------

/// Returns true if the video codec can be stream-copied into the target container.
pub(super) fn is_video_codec_compatible(codec: &Codec, container: ContainerFormat) -> bool {
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

/// Pre-flight check: reject codec/container combinations that cannot work.
fn validate_codec_container(
    codec: Option<&VideoCodecArg>,
    container: ContainerFormat,
) -> Result<()> {
    match (codec, container) {
        (Some(VideoCodecArg::H264), ContainerFormat::WebM) => Err(miette::miette!(
            "H.264 is not supported in WebM — use --codec av1 or choose an MP4/MKV output"
        )),
        (Some(VideoCodecArg::H265), ContainerFormat::WebM) => Err(miette::miette!(
            "H.265 is not supported in WebM — use --codec av1 or choose an MP4/MKV output"
        )),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Output QC
// ---------------------------------------------------------------------------

/// QC metadata extracted from the output file after muxing.
pub(super) struct OutputQc {
    pub codec: Option<String>,
    pub duration_secs: Option<f64>,
    pub bitrate_kbps: Option<u64>,
}

/// Probes the output file to extract QC metadata for the complete event.
pub(super) fn probe_output_qc(path: &Path) -> OutputQc {
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
// Re-encode path
// ---------------------------------------------------------------------------

pub(super) fn reencode(args: &ProcessArgs<'_>, json_mode: bool) -> Result<TranscodeOutput> {
    validate_output_format(args.output)?;

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

    let DemuxerWithConfigs {
        demuxer,
        video_tracks: video_track_configs,
        audio_tracks: audio_track_configs,
    } = open_demuxer_with_configs(args.input)?;

    // Determine target audio codec based on output container
    let out_container = output_container(args.output).unwrap_or(ContainerFormat::Mp4);

    // Validate codec-container compatibility before doing any work.
    validate_codec_container(args.codec, out_container)?;

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

    for vtc in &video_track_configs {
        match vtc.codec {
            VideoTrackCodec::H264 => {
                let decoder = H264Decoder::new(&vtc.config_data)
                    .into_diagnostic()
                    .wrap_err("failed to create H.264 decoder")?;
                builder = builder.with_decoder(vtc.track_index, decoder);
            }
            VideoTrackCodec::H265 => {
                let decoder = H265Decoder::new(&vtc.config_data)
                    .into_diagnostic()
                    .wrap_err("failed to create H.265 decoder")?;
                builder = builder.with_decoder(vtc.track_index, decoder);
            }
            VideoTrackCodec::Av1 => {
                let decoder = Av1Decoder::new(&vtc.config_data)
                    .into_diagnostic()
                    .wrap_err("failed to create AV1 decoder")?;
                builder = builder.with_decoder(vtc.track_index, decoder);
            }
        }

        // Select encoder based on output container and --codec flag
        let use_av1 = matches!(out_container, ContainerFormat::WebM)
            || matches!(args.codec, Some(VideoCodecArg::Av1));
        let use_h265 = matches!(args.codec, Some(VideoCodecArg::H265));

        // Determine encode dimensions (resize overrides source)
        let (enc_w, enc_h) = if let Some(resize_str) = args.resize {
            parse_resize(resize_str)?
        } else {
            (vtc.width, vtc.height)
        };

        if use_av1 {
            let mut enc_builder = Av1EncoderBuilder::new()
                .quality(quality_target)
                .track_index(vtc.track_index)
                .dimensions(enc_w, enc_h)
                .speed(6)
                .max_frame_rate(frame_rate_hint);

            if let Some(cs) = vtc.color_space {
                enc_builder = enc_builder.color_space(cs);
            }

            let encoder = enc_builder
                .build()
                .into_diagnostic()
                .wrap_err("failed to create AV1 encoder")?;

            builder = builder.with_encoder(vtc.track_index, encoder);
        } else if use_h265 {
            let mut enc_builder = H265EncoderBuilder::new()
                .quality(quality_target)
                .track_index(vtc.track_index)
                .dimensions(enc_w, enc_h)
                .max_frame_rate(frame_rate_hint);

            if let Some(cs) = vtc.color_space {
                enc_builder = enc_builder.color_space(cs);
            }

            let encoder = enc_builder
                .build()
                .into_diagnostic()
                .wrap_err("failed to create H.265 encoder")?;

            builder = builder
                .with_encoder(vtc.track_index, encoder)
                .with_output_codec(vtc.track_index, Codec::Video(VideoCodec::H265));
        } else {
            let mut enc_builder = H264EncoderBuilder::new()
                .quality(quality_target)
                .max_frame_rate(frame_rate_hint)
                .track_index(vtc.track_index);

            if let Some(cs) = vtc.color_space {
                enc_builder = enc_builder.color_space(cs);
            }

            let encoder = enc_builder
                .build()
                .into_diagnostic()
                .wrap_err("failed to create H.264 encoder")?;

            builder = builder.with_encoder(vtc.track_index, encoder);
        }

        // Add scale filter if --resize was specified
        if args.resize.is_some() {
            let aspect_mode = match args.aspect_mode_arg {
                AspectModeArg::Stretch => AspectMode::Stretch,
                AspectModeArg::Fit => AspectMode::Fit,
                AspectModeArg::Fill => AspectMode::Fill,
            };
            let scale_filter = ScaleFilter::new(enc_w, enc_h).with_aspect_mode(aspect_mode);
            builder = builder.with_filter(vtc.track_index, scale_filter);
            builder = builder.with_output_dimensions(vtc.track_index, enc_w, enc_h);
        }

        // Add crop filter if --crop was specified (applied after scale)
        if let Some(crop_str) = args.crop {
            let (cx, cy, cw, ch) = parse_crop(crop_str)?;
            let crop_filter = CropFilter::new(cx, cy, cw, ch)
                .into_diagnostic()
                .wrap_err("invalid crop parameters")?;
            builder = builder.with_filter(vtc.track_index, crop_filter);
            builder = builder.with_output_dimensions(vtc.track_index, cw, ch);
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
