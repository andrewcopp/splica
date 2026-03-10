//! Core media types: packets, frames, and track metadata.

use bytes::Bytes;

use crate::timestamp::Timestamp;

// ---------------------------------------------------------------------------
// Newtypes
// ---------------------------------------------------------------------------

/// Index of a track within a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrackIndex(pub u32);

// ---------------------------------------------------------------------------
// Codec identification
// ---------------------------------------------------------------------------

/// Supported video codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    H264,
    H265,
    Av1,
}

/// Supported audio codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioCodec {
    Aac,
    Opus,
}

/// A codec identifier (video or audio).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    Video(VideoCodec),
    Audio(AudioCodec),
}

// ---------------------------------------------------------------------------
// Pixel formats
// ---------------------------------------------------------------------------

/// Pixel format describing how video pixel data is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// 4:2:0 planar, 8-bit
    Yuv420p,
    /// 4:2:0 planar, 10-bit (stored in 16-bit words)
    Yuv420p10,
    /// 4:2:2 planar, 8-bit
    Yuv422p,
    /// 4:2:2 planar, 10-bit
    Yuv422p10,
    /// 4:4:4 planar, 8-bit
    Yuv444p,
    /// Packed RGBA, 8 bits per component
    Rgba,
    /// Single-component grayscale, 8-bit
    Gray8,
}

// ---------------------------------------------------------------------------
// Color space
// ---------------------------------------------------------------------------

/// Color primaries (defines the RGB gamut).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorPrimaries {
    Bt709,
    Bt2020,
    Smpte432,
}

/// Transfer characteristics (OETF/EOTF — gamma curve).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferCharacteristics {
    Bt709,
    Smpte2084,
    HybridLogGamma,
}

/// Matrix coefficients for YCbCr ↔ RGB conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatrixCoefficients {
    Bt709,
    Bt2020NonConstant,
    Bt2020Constant,
    Identity,
}

/// Full color space description for a video frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorSpace {
    pub primaries: ColorPrimaries,
    pub transfer: TransferCharacteristics,
    pub matrix: MatrixCoefficients,
}

impl ColorSpace {
    /// Standard BT.709 color space (SDR, HD).
    pub const BT709: Self = Self {
        primaries: ColorPrimaries::Bt709,
        transfer: TransferCharacteristics::Bt709,
        matrix: MatrixCoefficients::Bt709,
    };

    /// BT.2020 with PQ transfer (HDR10).
    pub const BT2020_PQ: Self = Self {
        primaries: ColorPrimaries::Bt2020,
        transfer: TransferCharacteristics::Smpte2084,
        matrix: MatrixCoefficients::Bt2020NonConstant,
    };

    /// BT.2020 with HLG transfer.
    pub const BT2020_HLG: Self = Self {
        primaries: ColorPrimaries::Bt2020,
        transfer: TransferCharacteristics::HybridLogGamma,
        matrix: MatrixCoefficients::Bt2020NonConstant,
    };
}

// ---------------------------------------------------------------------------
// Audio sample format
// ---------------------------------------------------------------------------

/// Sample format for audio data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleFormat {
    /// 16-bit signed integer, interleaved
    S16,
    /// 32-bit signed integer, interleaved
    S32,
    /// 32-bit float, interleaved
    F32,
    /// 32-bit float, planar (one plane per channel)
    F32Planar,
}

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
// VideoFrame
// ---------------------------------------------------------------------------

/// A decoded video frame with YUV plane data.
///
/// Plane data is stored as `bytes::Bytes` for ref-counted, zero-copy sharing.
/// Color space metadata is always present (never optional) so that color
/// handling is correct by default.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Pixel format.
    pub pixel_format: PixelFormat,
    /// Color space metadata.
    pub color_space: ColorSpace,
    /// Presentation timestamp.
    pub pts: Timestamp,
    /// Plane data. Number of planes depends on pixel format:
    /// - YUV formats: 3 planes (Y, U, V)
    /// - RGBA: 1 plane (packed)
    /// - Gray8: 1 plane
    pub planes: Vec<Bytes>,
    /// Row stride (bytes per row) for each plane.
    pub strides: Vec<u32>,
}

// ---------------------------------------------------------------------------
// AudioFrame
// ---------------------------------------------------------------------------

/// A decoded audio frame with PCM sample data.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// Sample rate in Hz (e.g., 44100, 48000).
    pub sample_rate: u32,
    /// Channel layout.
    pub channel_layout: ChannelLayout,
    /// Sample format.
    pub sample_format: SampleFormat,
    /// Number of samples per channel in this frame.
    pub sample_count: u32,
    /// Presentation timestamp.
    pub pts: Timestamp,
    /// Raw sample data. Layout depends on `sample_format`:
    /// - Interleaved formats: single `Bytes` with samples interleaved across channels
    /// - Planar formats: one `Bytes` per channel
    pub data: Vec<Bytes>,
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
    pub frame_rate: Option<f64>,
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
    fn test_that_packet_is_owned_and_cloneable() {
        // GIVEN
        let packet = Packet {
            track_index: TrackIndex(0),
            pts: Timestamp::new(0, 30),
            dts: Timestamp::new(0, 30),
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
        // GIVEN
        let frame = VideoFrame {
            width: 1920,
            height: 1080,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpace::BT709,
            pts: Timestamp::new(0, 30),
            planes: vec![
                Bytes::from(vec![0u8; 1920 * 1080]),
                Bytes::from(vec![128u8; 960 * 540]),
                Bytes::from(vec![128u8; 960 * 540]),
            ],
            strides: vec![1920, 960, 960],
        };

        // THEN — Elena can inspect color space directly
        assert_eq!(frame.color_space.primaries, ColorPrimaries::Bt709);
        assert_eq!(frame.color_space.transfer, TransferCharacteristics::Bt709);
        assert_eq!(frame.color_space.matrix, MatrixCoefficients::Bt709);
    }

    #[test]
    fn test_that_frame_enum_provides_pts() {
        // GIVEN
        let video_frame = VideoFrame {
            width: 320,
            height: 240,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpace::BT709,
            pts: Timestamp::new(90, 30),
            planes: vec![],
            strides: vec![],
        };
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
}
