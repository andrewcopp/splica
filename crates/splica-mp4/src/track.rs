//! Per-track metadata assembled from parsed MP4 boxes.

use splica_core::{
    AudioCodec, AudioTrackInfo, ChannelLayout, Codec, FrameRate, PixelFormat, SubtitleCodec,
    Timestamp, TrackIndex, TrackInfo, TrackKind, VideoCodec, VideoTrackInfo,
};

use crate::boxes::hdlr::HandlerType;
use crate::boxes::stsd::CodecConfig;
use crate::sample_table::SampleTable;

/// Internal representation of an MP4 track.
#[derive(Debug)]
pub(crate) struct Mp4Track {
    /// Retained: needed for muxer track-mapping and multi-track selection.
    #[allow(dead_code)]
    pub track_id: u32,
    pub handler_type: HandlerType,
    pub timescale: u32,
    pub duration: u64,
    pub codec_config: CodecConfig,
    pub sample_table: SampleTable,
    /// From tkhd (video only).
    pub width: u32,
    pub height: u32,
}

impl Mp4Track {
    pub fn is_video(&self) -> bool {
        self.handler_type == HandlerType::Video
    }

    pub fn is_audio(&self) -> bool {
        self.handler_type == HandlerType::Audio
    }

    pub fn is_subtitle(&self) -> bool {
        self.handler_type == HandlerType::Subtitle
    }

    pub fn to_track_info(&self, index: TrackIndex) -> TrackInfo {
        let kind = if self.is_video() {
            TrackKind::Video
        } else if self.is_subtitle() {
            TrackKind::Subtitle
        } else {
            TrackKind::Audio
        };

        let codec = match &self.codec_config {
            CodecConfig::Avc1 { .. } => Codec::Video(VideoCodec::H264),
            CodecConfig::Hev1 { .. } => Codec::Video(VideoCodec::H265),
            CodecConfig::Av1 { .. } => Codec::Video(VideoCodec::Av1),
            CodecConfig::Mp4a { .. } => Codec::Audio(AudioCodec::Aac),
            CodecConfig::Unknown(name) => {
                if self.is_video() {
                    Codec::Video(VideoCodec::Other(name.clone()))
                } else if self.is_subtitle() {
                    let sc = match name.as_str() {
                        "tx3g" | "stpp" => SubtitleCodec::Other(name.clone()),
                        "wvtt" => SubtitleCodec::WebVtt,
                        _ => SubtitleCodec::Other(name.clone()),
                    };
                    Codec::Subtitle(sc)
                } else {
                    Codec::Audio(AudioCodec::Other(name.clone()))
                }
            }
        };

        let duration = if self.timescale > 0 && self.duration > 0 {
            i64::try_from(self.duration)
                .ok()
                .and_then(|ticks| Timestamp::new(ticks, self.timescale))
        } else {
            None
        };

        let video = if self.is_video() {
            let (w, h, color_space) = match &self.codec_config {
                CodecConfig::Avc1 {
                    width,
                    height,
                    color_space,
                    ..
                }
                | CodecConfig::Hev1 {
                    width,
                    height,
                    color_space,
                    ..
                }
                | CodecConfig::Av1 {
                    width,
                    height,
                    color_space,
                    ..
                } => (*width as u32, *height as u32, *color_space),
                _ => (self.width, self.height, None),
            };

            let frame_rate = self.compute_frame_rate();

            let (profile, level) = self.extract_profile_level();
            let color_primaries = color_space.map(|cs| format_color_primaries(&cs));
            let transfer_characteristics =
                color_space.map(|cs| format_transfer_characteristics(&cs));
            let matrix_coefficients = color_space.map(|cs| format_matrix_coefficients(&cs));

            Some(VideoTrackInfo {
                width: w,
                height: h,
                pixel_format: Some(PixelFormat::Yuv420p),
                color_space,
                frame_rate,
                profile,
                level,
                color_primaries,
                transfer_characteristics,
                matrix_coefficients,
            })
        } else {
            None
        };

        let audio = if self.is_audio() {
            let (sample_rate, channel_count) = match &self.codec_config {
                CodecConfig::Mp4a {
                    sample_rate,
                    channel_count,
                    ..
                } => (*sample_rate, *channel_count),
                _ => (self.timescale, 2),
            };

            let channel_layout = match channel_count {
                1 => Some(ChannelLayout::Mono),
                2 => Some(ChannelLayout::Stereo),
                6 => Some(ChannelLayout::Surround5_1),
                8 => Some(ChannelLayout::Surround7_1),
                _ => None,
            };

            Some(AudioTrackInfo {
                sample_rate,
                channel_layout,
                sample_format: None, // Determined at decode time
            })
        } else {
            None
        };

        TrackInfo {
            index,
            kind,
            codec,
            duration,
            video,
            audio,
        }
    }

    /// Extracts codec profile and level from the raw codec configuration bytes.
    fn extract_profile_level(&self) -> (Option<String>, Option<String>) {
        match &self.codec_config {
            CodecConfig::Avc1 { avcc, .. } => {
                if avcc.len() >= 4 {
                    let profile_idc = avcc[1];
                    let level_idc = avcc[3];
                    let profile = format_h264_profile(profile_idc);
                    let level = format_h264_level(level_idc);
                    (Some(profile), Some(level))
                } else {
                    (None, None)
                }
            }
            CodecConfig::Hev1 { hvcc, .. } => {
                if hvcc.len() >= 13 {
                    let general_profile_idc = hvcc[1] & 0x1F;
                    let general_level_idc = hvcc[12];
                    let profile = format_h265_profile(general_profile_idc);
                    let level = format_h265_level(general_level_idc);
                    (Some(profile), Some(level))
                } else {
                    (None, None)
                }
            }
            CodecConfig::Av1 { av1c, .. } => {
                if av1c.len() >= 2 {
                    let seq_profile = (av1c[1] >> 5) & 0x07;
                    let seq_level_idx = av1c[1] & 0x1F;
                    let profile = format_av1_profile(seq_profile);
                    let level = format_av1_level(seq_level_idx);
                    (Some(profile), Some(level))
                } else {
                    (None, None)
                }
            }
            _ => (None, None),
        }
    }

    fn compute_frame_rate(&self) -> Option<FrameRate> {
        // Compute from sample table: total_duration / sample_count
        if self.sample_table.entries.is_empty() || self.timescale == 0 {
            return None;
        }

        let sample_count = self.sample_table.entries.len() as u64;
        if self.duration == 0 {
            return None;
        }

        // frame_rate = sample_count * timescale / duration
        // Express as rational: numerator = sample_count * timescale, denominator = duration
        let num = sample_count * self.timescale as u64;
        let den = self.duration;

        // Simplify to u32 range
        let g = gcd(num, den);
        let num = num / g;
        let den = den / g;

        u32::try_from(num)
            .ok()
            .and_then(|n| u32::try_from(den).ok().and_then(|d| FrameRate::new(n, d)))
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

// ---------------------------------------------------------------------------
// Codec profile/level formatting
// ---------------------------------------------------------------------------

fn format_h264_profile(profile_idc: u8) -> String {
    match profile_idc {
        66 => "Baseline".to_string(),
        77 => "Main".to_string(),
        88 => "Extended".to_string(),
        100 => "High".to_string(),
        110 => "High 10".to_string(),
        122 => "High 4:2:2".to_string(),
        244 => "High 4:4:4 Predictive".to_string(),
        _ => format!("Unknown ({profile_idc})"),
    }
}

fn format_h264_level(level_idc: u8) -> String {
    let major = level_idc / 10;
    let minor = level_idc % 10;
    format!("{major}.{minor}")
}

fn format_h265_profile(general_profile_idc: u8) -> String {
    match general_profile_idc {
        1 => "Main".to_string(),
        2 => "Main 10".to_string(),
        3 => "Main Still Picture".to_string(),
        4 => "Range Extensions".to_string(),
        _ => format!("Unknown ({general_profile_idc})"),
    }
}

fn format_h265_level(general_level_idc: u8) -> String {
    let major = general_level_idc / 30;
    let minor = (general_level_idc % 30) / 3;
    format!("{major}.{minor}")
}

fn format_av1_profile(seq_profile: u8) -> String {
    match seq_profile {
        0 => "Main".to_string(),
        1 => "High".to_string(),
        2 => "Professional".to_string(),
        _ => format!("Unknown ({seq_profile})"),
    }
}

fn format_av1_level(seq_level_idx: u8) -> String {
    let major = 2 + (seq_level_idx >> 2);
    let minor = seq_level_idx & 0x03;
    format!("{major}.{minor}")
}

// ---------------------------------------------------------------------------
// Color parameter formatting
// ---------------------------------------------------------------------------

fn format_color_primaries(cs: &splica_core::ColorSpace) -> String {
    use splica_core::media::ColorPrimaries;
    match cs.primaries {
        ColorPrimaries::Bt709 => "BT.709".to_string(),
        ColorPrimaries::Bt2020 => "BT.2020".to_string(),
        ColorPrimaries::Smpte432 => "SMPTE 432".to_string(),
    }
}

fn format_transfer_characteristics(cs: &splica_core::ColorSpace) -> String {
    use splica_core::media::TransferCharacteristics;
    match cs.transfer {
        TransferCharacteristics::Bt709 => "BT.709".to_string(),
        TransferCharacteristics::Smpte2084 => "SMPTE ST 2084".to_string(),
        TransferCharacteristics::HybridLogGamma => "HLG".to_string(),
    }
}

fn format_matrix_coefficients(cs: &splica_core::ColorSpace) -> String {
    use splica_core::media::MatrixCoefficients;
    match cs.matrix {
        MatrixCoefficients::Identity => "Identity".to_string(),
        MatrixCoefficients::Bt709 => "BT.709".to_string(),
        MatrixCoefficients::Bt2020NonConstant => "BT.2020 non-constant".to_string(),
        MatrixCoefficients::Bt2020Constant => "BT.2020 constant".to_string(),
    }
}
