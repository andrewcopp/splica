use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use miette::{Context, IntoDiagnostic, Result};

use splica_core::{Codec, ColorSpace, Demuxer, TrackIndex, TrackKind, VideoCodec};
use splica_filter::VolumeFilter;
use splica_mkv::MkvDemuxer;
use splica_mp4::boxes::stsd::CodecConfig;
use splica_mp4::Mp4Demuxer;
use splica_webm::WebmDemuxer;

use crate::commands::{
    detect_format, AspectModeArg, AudioCodecArg, DetectedFormat, EncodePreset, H264LevelArg,
    H264ProfileArg, VideoCodecArg,
};

// ---------------------------------------------------------------------------
// Process args
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
    pub audio_codec: Option<&'a AudioCodecArg>,
    pub audio_bitrate: Option<&'a str>,
    pub h264_profile: Option<&'a H264ProfileArg>,
    pub h264_level: Option<&'a H264LevelArg>,
}

// ---------------------------------------------------------------------------
// Video track config
// ---------------------------------------------------------------------------

/// Video codec configuration extracted from the container for re-encoding.
pub(super) enum VideoTrackCodec {
    H264,
    H265,
    Av1,
}

/// Named struct replacing the inner 6-tuple in `DemuxerWithConfigs`.
pub(super) struct VideoTrackConfig {
    pub track_index: TrackIndex,
    pub codec: VideoTrackCodec,
    pub config_data: Vec<u8>,
    pub color_space: Option<ColorSpace>,
    pub width: u32,
    pub height: u32,
}

/// Audio codec config extracted from the demuxer for audio tracks.
#[derive(Debug, Clone)]
pub(super) struct AudioCodecConfig {
    pub track_index: TrackIndex,
    pub codec: splica_core::AudioCodec,
    /// Raw codec-specific config (e.g., esds for AAC).
    pub config_data: Option<Vec<u8>>,
    pub sample_rate: u32,
    pub channel_layout: Option<splica_core::media::ChannelLayout>,
}

pub(super) struct DemuxerWithConfigs {
    pub demuxer: Box<dyn splica_core::Demuxer>,
    pub video_tracks: Vec<VideoTrackConfig>,
    pub audio_tracks: Vec<AudioCodecConfig>,
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parses a bitrate string like "2M", "1500k", or "1000000" into bits per second.
pub(super) fn parse_bitrate(s: &str) -> Result<u32> {
    let s = s.trim();
    let last = s.bytes().last().map(|b| b.to_ascii_uppercase());
    if let Some(b'M') = last {
        let prefix = &s[..s.len() - 1];
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000_000.0) as u32)
    } else if let Some(b'K') = last {
        let prefix = &s[..s.len() - 1];
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
pub(super) fn parse_resize(s: &str) -> Result<(u32, u32)> {
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
pub(super) fn parse_crop(s: &str) -> Result<(u32, u32, u32, u32)> {
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

pub(super) fn parse_volume(s: &str) -> Result<VolumeFilter> {
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
// Demuxer opening with codec configs
// ---------------------------------------------------------------------------

pub(super) fn open_demuxer_with_configs(input: &Path) -> Result<DemuxerWithConfigs> {
    let mut file = File::open(input)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open file '{}'", input.display()))?;
    let format = detect_format(&mut file)?;

    match format {
        DetectedFormat::Mp4 => open_mp4_configs(file),
        DetectedFormat::WebM => open_webm_configs(file),
        DetectedFormat::Mkv => open_mkv_configs(file),
    }
}

fn open_mp4_configs(file: File) -> Result<DemuxerWithConfigs> {
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
                        video_configs.push(VideoTrackConfig {
                            track_index: track.index,
                            codec: VideoTrackCodec::H264,
                            config_data: avcc.to_vec(),
                            color_space: *color_space,
                            width: *width as u32,
                            height: *height as u32,
                        });
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
                        video_configs.push(VideoTrackConfig {
                            track_index: track.index,
                            codec: VideoTrackCodec::H265,
                            config_data: hvcc.to_vec(),
                            color_space: *color_space,
                            width: *width as u32,
                            height: *height as u32,
                        });
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
                        video_configs.push(VideoTrackConfig {
                            track_index: track.index,
                            codec: VideoTrackCodec::Av1,
                            config_data: av1c.to_vec(),
                            color_space: *color_space,
                            width: *width as u32,
                            height: *height as u32,
                        });
                    }
                }
                _ => {}
            }
        }
        if track.kind == TrackKind::Audio {
            if let Codec::Audio(ref audio_codec) = track.codec {
                let config_data =
                    if let Some(CodecConfig::Mp4a { esds, .. }) = mp4.codec_config(track.index) {
                        Some(esds.to_vec())
                    } else {
                        None
                    };
                audio_configs.push(AudioCodecConfig {
                    track_index: track.index,
                    codec: audio_codec.clone(),
                    config_data,
                    sample_rate: track.audio.as_ref().map(|a| a.sample_rate).unwrap_or(44100),
                    channel_layout: track.audio.as_ref().and_then(|a| a.channel_layout),
                });
            }
        }
    }
    Ok(DemuxerWithConfigs {
        demuxer: Box::new(mp4),
        video_tracks: video_configs,
        audio_tracks: audio_configs,
    })
}

fn open_webm_configs(file: File) -> Result<DemuxerWithConfigs> {
    let webm = WebmDemuxer::open(BufReader::new(file))
        .into_diagnostic()
        .wrap_err("failed to parse WebM container")?;
    let tracks = webm.tracks().to_vec();
    let mut video_configs = Vec::new();
    let mut audio_configs = Vec::new();
    for track in &tracks {
        if track.kind == TrackKind::Video {
            let codec_tag = match &track.codec {
                Codec::Video(VideoCodec::H264) => Some(VideoTrackCodec::H264),
                Codec::Video(VideoCodec::H265) => Some(VideoTrackCodec::H265),
                Codec::Video(VideoCodec::Av1) => Some(VideoTrackCodec::Av1),
                _ => None,
            };
            if let Some(vtc) = codec_tag {
                let config_data = webm
                    .codec_private(track.index)
                    .map(|d| d.to_vec())
                    .unwrap_or_default();
                let video = track.video.as_ref();
                video_configs.push(VideoTrackConfig {
                    track_index: track.index,
                    codec: vtc,
                    config_data,
                    color_space: video.and_then(|v| v.color_space),
                    width: video.map(|v| v.width).unwrap_or(0),
                    height: video.map(|v| v.height).unwrap_or(0),
                });
            }
        }
        if track.kind == TrackKind::Audio {
            if let Codec::Audio(ref audio_codec) = track.codec {
                audio_configs.push(AudioCodecConfig {
                    track_index: track.index,
                    codec: audio_codec.clone(),
                    config_data: None,
                    sample_rate: track.audio.as_ref().map(|a| a.sample_rate).unwrap_or(48000),
                    channel_layout: track.audio.as_ref().and_then(|a| a.channel_layout),
                });
            }
        }
    }
    Ok(DemuxerWithConfigs {
        demuxer: Box::new(webm),
        video_tracks: video_configs,
        audio_tracks: audio_configs,
    })
}

fn open_mkv_configs(file: File) -> Result<DemuxerWithConfigs> {
    let mkv = MkvDemuxer::open(BufReader::new(file))
        .into_diagnostic()
        .wrap_err("failed to parse MKV container")?;
    let tracks = mkv.tracks().to_vec();
    let mut video_configs = Vec::new();
    let mut audio_configs = Vec::new();
    for track in &tracks {
        if track.kind == TrackKind::Video {
            let codec_tag = match &track.codec {
                Codec::Video(VideoCodec::H264) => Some(VideoTrackCodec::H264),
                Codec::Video(VideoCodec::H265) => Some(VideoTrackCodec::H265),
                Codec::Video(VideoCodec::Av1) => Some(VideoTrackCodec::Av1),
                _ => None,
            };
            if let Some(vtc) = codec_tag {
                let config_data = mkv
                    .codec_private(track.index)
                    .map(|d| d.to_vec())
                    .unwrap_or_default();
                let video = track.video.as_ref();
                video_configs.push(VideoTrackConfig {
                    track_index: track.index,
                    codec: vtc,
                    config_data,
                    color_space: video.and_then(|v| v.color_space),
                    width: video.map(|v| v.width).unwrap_or(0),
                    height: video.map(|v| v.height).unwrap_or(0),
                });
            }
        }
        if track.kind == TrackKind::Audio {
            if let Codec::Audio(ref audio_codec) = track.codec {
                audio_configs.push(AudioCodecConfig {
                    track_index: track.index,
                    codec: audio_codec.clone(),
                    config_data: None,
                    sample_rate: track.audio.as_ref().map(|a| a.sample_rate).unwrap_or(48000),
                    channel_layout: track.audio.as_ref().and_then(|a| a.channel_layout),
                });
            }
        }
    }
    Ok(DemuxerWithConfigs {
        demuxer: Box::new(mkv),
        video_tracks: video_configs,
        audio_tracks: audio_configs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // parse_bitrate
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_bitrate_parses_megabit_suffix_uppercase() {
        let bps = parse_bitrate("2M").unwrap();
        assert_eq!(bps, 2_000_000);
    }

    #[test]
    fn test_that_bitrate_parses_megabit_suffix_lowercase() {
        let bps = parse_bitrate("1.5m").unwrap();
        assert_eq!(bps, 1_500_000);
    }

    #[test]
    fn test_that_bitrate_parses_kilobit_suffix_lowercase() {
        let bps = parse_bitrate("1500k").unwrap();
        assert_eq!(bps, 1_500_000);
    }

    #[test]
    fn test_that_bitrate_parses_kilobit_suffix_uppercase() {
        let bps = parse_bitrate("800K").unwrap();
        assert_eq!(bps, 800_000);
    }

    #[test]
    fn test_that_bitrate_parses_raw_bps() {
        let bps = parse_bitrate("500000").unwrap();
        assert_eq!(bps, 500_000);
    }

    #[test]
    fn test_that_bitrate_trims_whitespace() {
        let bps = parse_bitrate("  2M  ").unwrap();
        assert_eq!(bps, 2_000_000);
    }

    #[test]
    fn test_that_bitrate_rejects_empty_string() {
        assert!(parse_bitrate("").is_err());
    }

    #[test]
    fn test_that_bitrate_rejects_non_numeric() {
        assert!(parse_bitrate("abcM").is_err());
    }

    #[test]
    fn test_that_bitrate_parses_fractional_kilobits() {
        let bps = parse_bitrate("1.5k").unwrap();
        assert_eq!(bps, 1_500);
    }

    // -----------------------------------------------------------------------
    // parse_resize
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_resize_parses_valid_dimensions() {
        let (w, h) = parse_resize("1280x720").unwrap();
        assert_eq!((w, h), (1280, 720));
    }

    #[test]
    fn test_that_resize_rejects_zero_width() {
        assert!(parse_resize("0x720").is_err());
    }

    #[test]
    fn test_that_resize_rejects_zero_height() {
        assert!(parse_resize("1280x0").is_err());
    }

    #[test]
    fn test_that_resize_rejects_missing_separator() {
        assert!(parse_resize("1280720").is_err());
    }

    #[test]
    fn test_that_resize_rejects_extra_dimensions() {
        assert!(parse_resize("1280x720x480").is_err());
    }

    #[test]
    fn test_that_resize_rejects_negative_values() {
        assert!(parse_resize("-1x720").is_err());
    }

    #[test]
    fn test_that_resize_rejects_non_numeric() {
        assert!(parse_resize("widexhigh").is_err());
    }

    // -----------------------------------------------------------------------
    // parse_crop
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_crop_parses_valid_geometry() {
        let (x, y, w, h) = parse_crop("1080x1080+420+0").unwrap();
        assert_eq!((x, y, w, h), (420, 0, 1080, 1080));
    }

    #[test]
    fn test_that_crop_rejects_zero_width() {
        assert!(parse_crop("0x1080+420+0").is_err());
    }

    #[test]
    fn test_that_crop_rejects_zero_height() {
        assert!(parse_crop("1080x0+420+0").is_err());
    }

    #[test]
    fn test_that_crop_rejects_missing_offsets() {
        assert!(parse_crop("1080x1080").is_err());
    }

    #[test]
    fn test_that_crop_rejects_missing_y_offset() {
        assert!(parse_crop("1080x1080+420").is_err());
    }

    #[test]
    fn test_that_crop_rejects_no_separator() {
        assert!(parse_crop("1080").is_err());
    }

    #[test]
    fn test_that_crop_allows_zero_offsets() {
        let (x, y, w, h) = parse_crop("640x480+0+0").unwrap();
        assert_eq!((x, y, w, h), (0, 0, 640, 480));
    }

    // -----------------------------------------------------------------------
    // parse_volume
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_volume_parses_linear_gain() {
        let vol = parse_volume("0.5").unwrap();
        assert!((vol.gain() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_that_volume_parses_db_uppercase_suffix() {
        let vol = parse_volume("-6dB").unwrap();
        let expected = 10f32.powf(-6.0 / 20.0);
        assert!((vol.gain() - expected).abs() < 0.001);
    }

    #[test]
    fn test_that_volume_parses_db_lowercase_suffix() {
        let vol = parse_volume("-6db").unwrap();
        let expected = 10f32.powf(-6.0 / 20.0);
        assert!((vol.gain() - expected).abs() < 0.001);
    }

    #[test]
    fn test_that_volume_parses_positive_db() {
        let vol = parse_volume("+3dB").unwrap();
        let expected = 10f32.powf(3.0 / 20.0);
        assert!((vol.gain() - expected).abs() < 0.001);
    }

    #[test]
    fn test_that_volume_parses_zero_db() {
        let vol = parse_volume("0dB").unwrap();
        assert!((vol.gain() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_that_volume_rejects_negative_linear_gain() {
        assert!(parse_volume("-1.0").is_err());
    }

    #[test]
    fn test_that_volume_trims_whitespace() {
        let vol = parse_volume("  1.0  ").unwrap();
        assert!((vol.gain() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_that_volume_rejects_non_numeric() {
        assert!(parse_volume("loud").is_err());
    }

    #[test]
    fn test_that_volume_unity_gain() {
        let vol = parse_volume("1.0").unwrap();
        assert!((vol.gain() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_that_volume_double_gain() {
        let vol = parse_volume("2.0").unwrap();
        assert!((vol.gain() - 2.0).abs() < f32::EPSILON);
    }
}
