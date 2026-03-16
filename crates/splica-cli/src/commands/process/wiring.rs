//! Encoder/decoder wiring helpers for the re-encode pipeline.
//!
//! These functions create decoders, encoders, and filters for individual tracks
//! and attach them to a `PipelineBuilder`.

use miette::{Context, IntoDiagnostic, Result};

use splica_codec::{
    AacDecoder, AacEncoderBuilder, Av1Decoder, Av1EncoderBuilder, H264Decoder, H264EncoderBuilder,
    H264EncoderLevel, H264EncoderProfile, H265Decoder, H265EncoderBuilder, OpusDecoder,
    OpusEncoderBuilder,
};
use splica_core::{AudioCodec, Codec, ContainerFormat, VideoCodec};
use splica_filter::{AspectMode, CropFilter, ScaleFilter};
use splica_pipeline::PipelineBuilder;

use super::args::{
    parse_crop, parse_resize, AudioCodecConfig, ProcessArgs, VideoTrackCodec, VideoTrackConfig,
};
use crate::commands::{AspectModeArg, H264LevelArg, H264ProfileArg, VideoCodecArg};

// ---------------------------------------------------------------------------
// Video encoder wiring
// ---------------------------------------------------------------------------

/// Creates the video decoder, encoder, and optional filters for a single video
/// track and attaches them to the pipeline builder.
pub(super) fn wire_video_encoder(
    mut builder: PipelineBuilder,
    vtc: &VideoTrackConfig,
    args: &ProcessArgs<'_>,
    out_container: ContainerFormat,
    quality_target: splica_core::QualityTarget,
    frame_rate_hint: f32,
) -> Result<PipelineBuilder> {
    // Decoder
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

    // Encoder — select based on output container and --codec flag
    let use_av1 = matches!(out_container, ContainerFormat::WebM)
        || matches!(args.codec, Some(VideoCodecArg::Av1));
    let use_h265 = matches!(args.codec, Some(VideoCodecArg::H265));

    // Validate that --h264-profile/--h264-level are only used with H.264 output
    if use_av1 || use_h265 {
        if args.h264_profile.is_some() {
            return Err(miette::miette!(
                "--h264-profile can only be used when the output codec is H.264"
            ));
        }
        if args.h264_level.is_some() {
            return Err(miette::miette!(
                "--h264-level can only be used when the output codec is H.264"
            ));
        }
    }

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
            .speed(match args.preset {
                Some(crate::commands::EncodePreset::Fast) => 8,
                None | Some(crate::commands::EncodePreset::Medium) => 6,
                Some(crate::commands::EncodePreset::Slow) => 4,
            })
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

        if let Some(profile_arg) = args.h264_profile {
            let profile = match profile_arg {
                H264ProfileArg::Baseline => H264EncoderProfile::Baseline,
                H264ProfileArg::Main => H264EncoderProfile::Main,
                H264ProfileArg::High => H264EncoderProfile::High,
            };
            enc_builder = enc_builder.profile(profile);
        }

        if let Some(level_arg) = args.h264_level {
            let level = match level_arg {
                H264LevelArg::L3_0 => H264EncoderLevel::Level3_0,
                H264LevelArg::L3_1 => H264EncoderLevel::Level3_1,
                H264LevelArg::L4_0 => H264EncoderLevel::Level4_0,
                H264LevelArg::L4_1 => H264EncoderLevel::Level4_1,
                H264LevelArg::L5_0 => H264EncoderLevel::Level5_0,
                H264LevelArg::L5_1 => H264EncoderLevel::Level5_1,
            };
            enc_builder = enc_builder.level(level);
        }

        let encoder = enc_builder
            .build()
            .into_diagnostic()
            .wrap_err("failed to create H.264 encoder")?;

        builder = builder.with_encoder(vtc.track_index, encoder);
    }

    // Scale filter (--resize)
    if args.resize.is_some() {
        let aspect_mode = match args.aspect_mode_arg {
            AspectModeArg::Stretch => AspectMode::Stretch,
            AspectModeArg::Fit => AspectMode::Fit,
            AspectModeArg::Fill => AspectMode::Fill,
        };
        let scale_filter = ScaleFilter::new(enc_w, enc_h).with_aspect_mode(aspect_mode);
        builder = builder.with_filter(vtc.track_index, scale_filter);
    }

    // Tell the muxer about post-resize dimensions for correct container metadata
    if args.resize.is_some() {
        builder = builder.with_output_dimensions(vtc.track_index, enc_w, enc_h);
    }

    // Crop filter (applied after scale)
    if let Some(crop_str) = args.crop {
        let (cx, cy, cw, ch) = parse_crop(crop_str)?;
        let crop_filter = CropFilter::new(cx, cy, cw, ch)
            .into_diagnostic()
            .wrap_err("invalid crop parameters")?;
        builder = builder.with_filter(vtc.track_index, crop_filter);
    }

    Ok(builder)
}

// ---------------------------------------------------------------------------
// Audio codec wiring
// ---------------------------------------------------------------------------

/// Creates the audio decoder and encoder for a single audio track that needs
/// transcoding and attaches them to the pipeline builder.
pub(super) fn wire_audio_codec(
    mut builder: PipelineBuilder,
    ac: &AudioCodecConfig,
    target_audio_codec: &AudioCodec,
    audio_bitrate: u32,
) -> Result<PipelineBuilder> {
    // Decoder
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

    // Encoder
    let channel_layout = ac
        .channel_layout
        .unwrap_or(splica_core::media::ChannelLayout::Stereo);

    match target_audio_codec {
        AudioCodec::Aac => {
            let encoder = AacEncoderBuilder::new()
                .bitrate(audio_bitrate)
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
                .bitrate(audio_bitrate)
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

    Ok(builder)
}
