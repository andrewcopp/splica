//! Core media types: packets, frames, and track metadata.

mod color;
mod frame;

pub use color::*;
pub use frame::*;

use bytes::Bytes;

use crate::timestamp::Timestamp;

// ---------------------------------------------------------------------------
// Newtypes
// ---------------------------------------------------------------------------

/// Index of a track within a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrackIndex(pub u32);

/// Rational frame rate represented as numerator/denominator.
///
/// This avoids floating-point imprecision for standard broadcast rates:
/// - 24000/1001 (23.976fps NTSC film)
/// - 30000/1001 (29.97fps NTSC)
/// - 60000/1001 (59.94fps)
///
/// Using `f64` cannot represent these exactly, causing drift in timecode calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameRate {
    pub numerator: u32,
    pub denominator: u32,
}

impl FrameRate {
    /// Creates a new frame rate. Returns `None` if denominator is zero.
    pub fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        Some(Self {
            numerator,
            denominator,
        })
    }

    /// Returns the frame rate as a floating-point value.
    pub fn as_f64(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }
}

impl std::fmt::Display for FrameRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.denominator == 1 {
            write!(f, "{}", self.numerator)
        } else {
            write!(f, "{}/{}", self.numerator, self.denominator)
        }
    }
}

// ---------------------------------------------------------------------------
// Resource budget
// ---------------------------------------------------------------------------

/// Resource limits for demuxer and pipeline operations.
///
/// Prevents unbounded memory allocation when processing untrusted input.
/// Pass to demuxer constructors to enforce limits in the I/O layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceBudget {
    /// Maximum total bytes the demuxer may buffer (e.g., moov box, sample data).
    pub max_bytes: u64,
    /// Maximum number of frames/packets to read. `None` means unlimited.
    pub max_frames: Option<u64>,
}

impl ResourceBudget {
    /// Creates a new resource budget.
    pub fn new(max_bytes: u64) -> Self {
        Self {
            max_bytes,
            max_frames: None,
        }
    }

    /// Sets the maximum number of frames.
    pub fn with_max_frames(mut self, max_frames: u64) -> Self {
        self.max_frames = Some(max_frames);
        self
    }
}

// ---------------------------------------------------------------------------
// Quality target
// ---------------------------------------------------------------------------

/// Encoder quality target: either perceptual quality (CRF) or explicit bitrate.
///
/// This is the shared type that the CLI passes through to codec-specific
/// encoders. Each encoder maps it to its native quality controls:
/// - H.264 (openh264): CRF is mapped to a bitrate estimate (openh264 lacks
///   native CRF/QP control via its safe API).
/// - AV1 (rav1e): CRF is mapped to the rav1e quantizer (0–255).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QualityTarget {
    /// Constant Rate Factor — perceptual quality target.
    ///
    /// Range: 0 (best quality) to 51 (smallest file). Default: 23.
    /// Matches the x264 CRF scale that most users are familiar with.
    Crf(u8),
    /// Target bitrate in bits per second.
    Bitrate(u32),
}

impl QualityTarget {
    /// Maximum CRF value (worst quality, smallest file).
    pub const MAX_CRF: u8 = 51;

    /// Default CRF value (visually transparent for most content).
    pub const DEFAULT_CRF: u8 = 23;

    /// Validates a CRF value is in the 0–51 range.
    pub fn crf(value: u8) -> Option<Self> {
        if value > Self::MAX_CRF {
            None
        } else {
            Some(Self::Crf(value))
        }
    }
}

// ---------------------------------------------------------------------------
// Container format
// ---------------------------------------------------------------------------

/// Container format for muxed output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerFormat {
    Mp4,
    WebM,
    Mkv,
}

impl ContainerFormat {
    /// Returns the `ContainerFormat` for a given file extension (case-insensitive).
    ///
    /// Returns `None` for unrecognized extensions.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "mp4" | "m4v" | "m4a" => Some(Self::Mp4),
            "webm" => Some(Self::WebM),
            "mkv" | "mka" => Some(Self::Mkv),
            _ => None,
        }
    }

    /// Returns true if splica currently supports writing this format.
    pub fn is_writable(self) -> bool {
        match self {
            Self::Mp4 | Self::WebM => true,
            Self::Mkv => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Codec identification
// ---------------------------------------------------------------------------

/// Supported video codecs.
///
/// Includes an `Other` variant for codecs not directly supported by splica
/// (e.g., ProRes, DNxHD). This prevents the enum from being a breaking change
/// when new codecs are added.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    H264,
    H265,
    Av1,
    /// A codec not directly supported by splica.
    Other(String),
}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoCodec::H264 => f.write_str("H.264"),
            VideoCodec::H265 => f.write_str("H.265"),
            VideoCodec::Av1 => f.write_str("AV1"),
            VideoCodec::Other(s) => f.write_str(s),
        }
    }
}

/// Supported audio codecs.
///
/// Includes an `Other` variant for codecs not directly supported by splica.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AudioCodec {
    Aac,
    Opus,
    /// A codec not directly supported by splica.
    Other(String),
}

impl std::fmt::Display for AudioCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioCodec::Aac => f.write_str("AAC"),
            AudioCodec::Opus => f.write_str("Opus"),
            AudioCodec::Other(s) => f.write_str(s),
        }
    }
}

/// Supported subtitle codecs.
///
/// Subtitles are always passed through (stream copy) — splica does not
/// decode or re-encode subtitle tracks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubtitleCodec {
    /// SubRip / SRT text subtitles.
    Srt,
    /// WebVTT text subtitles.
    WebVtt,
    /// A subtitle codec not directly known to splica.
    Other(String),
}

impl std::fmt::Display for SubtitleCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubtitleCodec::Srt => f.write_str("SRT"),
            SubtitleCodec::WebVtt => f.write_str("WebVTT"),
            SubtitleCodec::Other(s) => f.write_str(s),
        }
    }
}

/// A codec identifier (video, audio, or subtitle).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Codec {
    Video(VideoCodec),
    Audio(AudioCodec),
    Subtitle(SubtitleCodec),
}

impl std::fmt::Display for Codec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Codec::Video(vc) => vc.fmt(f),
            Codec::Audio(ac) => ac.fmt(f),
            Codec::Subtitle(sc) => sc.fmt(f),
        }
    }
}

// ---------------------------------------------------------------------------
// Channel layout
// ---------------------------------------------------------------------------

/// Channel layout for audio data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Surround5_1,
    Surround7_1,
}

impl ChannelLayout {
    /// Returns the number of channels in this layout.
    pub fn channel_count(self) -> u32 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
            Self::Surround5_1 => 6,
            Self::Surround7_1 => 8,
        }
    }
}

// ---------------------------------------------------------------------------
// Packet
// ---------------------------------------------------------------------------

/// A compressed media packet read from a container.
///
/// Owned data via `bytes::Bytes` — no lifetime coupling to the demuxer
/// that produced it. Supports zero-copy slicing and ref-counted sharing.
#[derive(Debug, Clone)]
pub struct Packet {
    /// Which track this packet belongs to.
    pub track_index: TrackIndex,
    /// Presentation timestamp (when to display this packet's decoded frame).
    pub pts: Timestamp,
    /// Decode timestamp (when to decode this packet). May differ from `pts` for B-frames.
    pub dts: Timestamp,
    /// Whether this packet starts at a keyframe (random access point).
    pub is_keyframe: bool,
    /// The compressed data.
    pub data: Bytes,
}

// ---------------------------------------------------------------------------
// Frame (pipeline transport type)
// ---------------------------------------------------------------------------

/// A decoded frame, either video or audio.
///
/// Used by the pipeline for routing frames between stages.
/// Filters operate on `VideoFrame` or `AudioFrame` directly — they
/// never see this enum.
#[derive(Debug, Clone)]
pub enum Frame {
    Video(VideoFrame),
    Audio(AudioFrame),
}

impl Frame {
    /// Returns the presentation timestamp of this frame.
    pub fn pts(&self) -> Timestamp {
        match self {
            Self::Video(f) => f.pts,
            Self::Audio(f) => f.pts,
        }
    }
}

// ---------------------------------------------------------------------------
// TrackInfo
// ---------------------------------------------------------------------------

/// The kind of media a track contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
}

/// Metadata about a single track in a container.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    /// Index of this track.
    pub index: TrackIndex,
    /// What kind of track this is.
    pub kind: TrackKind,
    /// Codec used by this track.
    pub codec: Codec,
    /// Duration of this track, if known.
    pub duration: Option<Timestamp>,
    /// Video-specific metadata (present only for video tracks).
    pub video: Option<VideoTrackInfo>,
    /// Audio-specific metadata (present only for audio tracks).
    pub audio: Option<AudioTrackInfo>,
}

/// Video-specific track metadata.
#[derive(Debug, Clone)]
pub struct VideoTrackInfo {
    pub width: u32,
    pub height: u32,
    pub pixel_format: Option<PixelFormat>,
    pub color_space: Option<ColorSpace>,
    pub frame_rate: Option<FrameRate>,
}

/// Audio-specific track metadata.
#[derive(Debug, Clone)]
pub struct AudioTrackInfo {
    pub sample_rate: u32,
    pub channel_layout: Option<ChannelLayout>,
    pub sample_format: Option<SampleFormat>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_container_format_from_extension_recognizes_mp4_variants() {
        assert_eq!(
            ContainerFormat::from_extension("mp4"),
            Some(ContainerFormat::Mp4)
        );
        assert_eq!(
            ContainerFormat::from_extension("m4v"),
            Some(ContainerFormat::Mp4)
        );
        assert_eq!(
            ContainerFormat::from_extension("m4a"),
            Some(ContainerFormat::Mp4)
        );
        assert_eq!(
            ContainerFormat::from_extension("MP4"),
            Some(ContainerFormat::Mp4)
        );
    }

    #[test]
    fn test_that_container_format_from_extension_recognizes_webm() {
        assert_eq!(
            ContainerFormat::from_extension("webm"),
            Some(ContainerFormat::WebM)
        );
        assert_eq!(
            ContainerFormat::from_extension("WEBM"),
            Some(ContainerFormat::WebM)
        );
    }

    #[test]
    fn test_that_container_format_from_extension_recognizes_mkv_variants() {
        assert_eq!(
            ContainerFormat::from_extension("mkv"),
            Some(ContainerFormat::Mkv)
        );
        assert_eq!(
            ContainerFormat::from_extension("mka"),
            Some(ContainerFormat::Mkv)
        );
    }

    #[test]
    fn test_that_container_format_from_extension_returns_none_for_unknown() {
        assert_eq!(ContainerFormat::from_extension("avi"), None);
        assert_eq!(ContainerFormat::from_extension(""), None);
    }

    #[test]
    fn test_that_all_formats_are_writable() {
        assert!(ContainerFormat::Mp4.is_writable());
        assert!(ContainerFormat::WebM.is_writable());
        assert!(ContainerFormat::Mkv.is_writable());
    }

    #[test]
    fn test_that_packet_is_owned_and_cloneable() {
        // GIVEN
        let packet = Packet {
            track_index: TrackIndex(0),
            pts: Timestamp::new(0, 30).unwrap(),
            dts: Timestamp::new(0, 30).unwrap(),
            is_keyframe: true,
            data: Bytes::from_static(b"compressed data"),
        };

        // WHEN — clone (simulating undo stack push)
        let cloned = packet.clone();

        // THEN — both are independent values
        assert_eq!(cloned.track_index, TrackIndex(0));
        assert!(cloned.is_keyframe);
    }

    #[test]
    fn test_that_video_frame_has_color_space_metadata() {
        // GIVEN — 1080p YUV420 in a single contiguous buffer
        let y_size = 1920 * 1080;
        let uv_size = 960 * 540;
        let total = y_size + 2 * uv_size;
        let mut buf = vec![0u8; total];
        buf[y_size..y_size + uv_size].fill(128);
        buf[y_size + uv_size..].fill(128);

        let frame = VideoFrame::new(
            1920,
            1080,
            PixelFormat::Yuv420p,
            Some(ColorSpace::BT709),
            Timestamp::new(0, 30).unwrap(),
            Bytes::from(buf),
            vec![
                PlaneLayout {
                    offset: 0,
                    stride: 1920,
                    width: 1920,
                    height: 1080,
                },
                PlaneLayout {
                    offset: y_size,
                    stride: 960,
                    width: 960,
                    height: 540,
                },
                PlaneLayout {
                    offset: y_size + uv_size,
                    stride: 960,
                    width: 960,
                    height: 540,
                },
            ],
        )
        .unwrap();

        // THEN — Elena can inspect color space directly
        let cs = frame.color_space.unwrap();
        assert_eq!(cs.primaries, ColorPrimaries::Bt709);
        assert_eq!(cs.transfer, TransferCharacteristics::Bt709);
        assert_eq!(cs.matrix, MatrixCoefficients::Bt709);
    }

    #[test]
    fn test_that_frame_enum_provides_pts() {
        // GIVEN
        let video_frame = VideoFrame::new(
            320,
            240,
            PixelFormat::Yuv420p,
            Some(ColorSpace::BT709),
            Timestamp::new(90, 30).unwrap(),
            Bytes::new(),
            vec![],
        )
        .unwrap();
        let frame = Frame::Video(video_frame);

        // WHEN
        let pts = frame.pts();

        // THEN — 3 seconds
        assert!((pts.as_seconds_f64() - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_that_channel_layout_reports_correct_count() {
        assert_eq!(ChannelLayout::Mono.channel_count(), 1);
        assert_eq!(ChannelLayout::Stereo.channel_count(), 2);
        assert_eq!(ChannelLayout::Surround5_1.channel_count(), 6);
        assert_eq!(ChannelLayout::Surround7_1.channel_count(), 8);
    }

    #[test]
    fn test_that_frame_rate_represents_ntsc_exactly() {
        // GIVEN — 29.97fps NTSC as rational
        let rate = FrameRate::new(30000, 1001).unwrap();

        // THEN — exact representation, no floating-point drift
        assert_eq!(rate.numerator, 30000);
        assert_eq!(rate.denominator, 1001);
        assert!((rate.as_f64() - 29.97002997).abs() < 1e-6);
        assert_eq!(rate.to_string(), "30000/1001");
    }

    #[test]
    fn test_that_frame_rate_displays_integer_without_denominator() {
        // GIVEN — exact 30fps
        let rate = FrameRate::new(30, 1).unwrap();

        // THEN
        assert_eq!(rate.to_string(), "30");
    }

    #[test]
    fn test_that_color_space_includes_range() {
        // GIVEN — BT.709 default is limited range (broadcast)
        let cs = ColorSpace::BT709;

        // THEN
        assert_eq!(cs.range, ColorRange::Limited);
    }

    #[test]
    fn test_that_video_codec_other_accepts_prores() {
        // GIVEN — Elena needs ProRes support without breaking changes
        let codec = VideoCodec::Other("ProRes".to_string());

        // THEN
        assert_eq!(codec, VideoCodec::Other("ProRes".to_string()));
    }

    #[test]
    fn test_that_audio_codec_other_accepts_flac() {
        // GIVEN
        let codec = AudioCodec::Other("FLAC".to_string());

        // THEN
        assert_eq!(codec, AudioCodec::Other("FLAC".to_string()));
    }

    #[test]
    fn test_that_video_frame_roundtrips_yuv420_plane_data() {
        // GIVEN — a 4x2 YUV420p frame (Y: 4x2, U: 2x1, V: 2x1)
        let y_data = [10u8, 20, 30, 40, 50, 60, 70, 80]; // 4x2
        let u_data = [128u8, 129]; // 2x1
        let v_data = [130u8, 131]; // 2x1

        let mut buf = Vec::with_capacity(y_data.len() + u_data.len() + v_data.len());
        buf.extend_from_slice(&y_data);
        buf.extend_from_slice(&u_data);
        buf.extend_from_slice(&v_data);

        let y_offset = 0;
        let u_offset = y_data.len();
        let v_offset = u_offset + u_data.len();

        // WHEN
        let frame = VideoFrame::new(
            4,
            2,
            PixelFormat::Yuv420p,
            Some(ColorSpace::BT709),
            Timestamp::new(0, 30).unwrap(),
            Bytes::from(buf),
            vec![
                PlaneLayout {
                    offset: y_offset,
                    stride: 4,
                    width: 4,
                    height: 2,
                },
                PlaneLayout {
                    offset: u_offset,
                    stride: 2,
                    width: 2,
                    height: 1,
                },
                PlaneLayout {
                    offset: v_offset,
                    stride: 2,
                    width: 2,
                    height: 1,
                },
            ],
        )
        .unwrap();

        // THEN — plane data round-trips without extra allocation
        assert_eq!(frame.plane_data(0).unwrap(), &y_data);
        assert_eq!(frame.plane_data(1).unwrap(), &u_data);
        assert_eq!(frame.plane_data(2).unwrap(), &v_data);
        assert!(frame.plane_data(3).is_none());
    }

    #[test]
    fn test_that_video_frame_rejects_out_of_bounds_plane() {
        // GIVEN — a plane layout that extends past the buffer
        let buf = Bytes::from(vec![0u8; 100]);

        // WHEN
        let result = VideoFrame::new(
            10,
            10,
            PixelFormat::Yuv420p,
            Some(ColorSpace::BT709),
            Timestamp::new(0, 30).unwrap(),
            buf,
            vec![PlaneLayout {
                offset: 0,
                stride: 10,
                width: 10,
                height: 20,
            }],
        );

        // THEN
        assert!(matches!(
            result,
            Err(VideoFrameError::PlaneOutOfBounds { .. })
        ));
    }

    #[test]
    fn test_that_video_frame_rejects_stride_smaller_than_width() {
        // GIVEN — stride < width
        let buf = Bytes::from(vec![0u8; 100]);

        // WHEN
        let result = VideoFrame::new(
            10,
            5,
            PixelFormat::Yuv420p,
            Some(ColorSpace::BT709),
            Timestamp::new(0, 30).unwrap(),
            buf,
            vec![PlaneLayout {
                offset: 0,
                stride: 5,
                width: 10,
                height: 5,
            }],
        );

        // THEN
        assert!(matches!(
            result,
            Err(VideoFrameError::StrideTooSmall { .. })
        ));
    }

    #[test]
    fn test_that_quality_target_crf_accepts_valid_range() {
        assert_eq!(QualityTarget::crf(0), Some(QualityTarget::Crf(0)));
        assert_eq!(QualityTarget::crf(23), Some(QualityTarget::Crf(23)));
        assert_eq!(QualityTarget::crf(51), Some(QualityTarget::Crf(51)));
    }

    #[test]
    fn test_that_quality_target_crf_rejects_out_of_range() {
        assert_eq!(QualityTarget::crf(52), None);
        assert_eq!(QualityTarget::crf(255), None);
    }

    #[test]
    fn test_that_quality_target_bitrate_stores_value() {
        let qt = QualityTarget::Bitrate(2_000_000);

        assert_eq!(qt, QualityTarget::Bitrate(2_000_000));
    }
}
