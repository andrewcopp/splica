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
#[allow(dead_code)]
pub(crate) struct Mp4Track {
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

            Some(VideoTrackInfo {
                width: w,
                height: h,
                pixel_format: Some(PixelFormat::Yuv420p),
                color_space,
                frame_rate,
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
