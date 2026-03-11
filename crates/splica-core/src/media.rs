//! Core media types: packets, frames, and track metadata.

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

/// A codec identifier (video or audio).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

/// Color range: limited (broadcast/studio) vs full (PC/JPEG).
///
/// Getting this wrong causes crushed blacks or blown highlights in every
/// downstream decoder that respects the flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorRange {
    /// Limited range (16–235 luma, 16–240 chroma for 8-bit). Broadcast standard.
    Limited,
    /// Full range (0–255 for 8-bit). Common in JPEG, screen capture, PC content.
    Full,
}

/// Full color space description for a video frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorSpace {
    pub primaries: ColorPrimaries,
    pub transfer: TransferCharacteristics,
    pub matrix: MatrixCoefficients,
    pub range: ColorRange,
}

impl ColorSpace {
    /// Standard BT.709 color space (SDR, HD, limited range).
    pub const BT709: Self = Self {
        primaries: ColorPrimaries::Bt709,
        transfer: TransferCharacteristics::Bt709,
        matrix: MatrixCoefficients::Bt709,
        range: ColorRange::Limited,
    };

    /// BT.2020 with PQ transfer (HDR10, limited range).
    pub const BT2020_PQ: Self = Self {
        primaries: ColorPrimaries::Bt2020,
        transfer: TransferCharacteristics::Smpte2084,
        matrix: MatrixCoefficients::Bt2020NonConstant,
        range: ColorRange::Limited,
    };

    /// BT.2020 with HLG transfer (limited range).
    pub const BT2020_HLG: Self = Self {
        primaries: ColorPrimaries::Bt2020,
        transfer: TransferCharacteristics::HybridLogGamma,
        matrix: MatrixCoefficients::Bt2020NonConstant,
        range: ColorRange::Limited,
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

/// Layout of a single plane within a contiguous frame buffer.
///
/// All offsets are relative to the start of the `VideoFrame::data` buffer.
/// This struct bundles the information that was previously split across
/// separate `planes` and `strides` vectors, eliminating the footgun where
/// `strides.len() != planes.len()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneLayout {
    /// Byte offset of this plane's first row within the frame buffer.
    pub offset: usize,
    /// Bytes per row (includes padding for alignment).
    pub stride: usize,
    /// Width of this plane in pixels.
    pub width: u32,
    /// Height of this plane in rows.
    pub height: u32,
}

impl PlaneLayout {
    /// Total bytes this plane occupies: `stride * height`.
    pub fn size(&self) -> usize {
        self.stride * self.height as usize
    }
}

/// A decoded video frame with pixel data in a single contiguous buffer.
///
/// All plane data lives in one `Bytes` allocation with `PlaneLayout` descriptors
/// indexing into it. This gives cache-friendly access, a single heap allocation,
/// and enables zero-copy handoff to WASM (one `ArrayBuffer` view).
///
/// Color space metadata is always present (never optional) so that color
/// handling is correct by default.
///
/// # WASM memory model
///
/// The single-buffer design is chosen specifically for efficient WASM interop.
/// The memory contract across the WASM boundary is:
///
/// 1. **Copy-out model** (chosen): `as_wasm_buffer()` returns a `&[u8]` view of
///    the frame's contiguous buffer. On the JS side, `wasm-bindgen` copies this
///    into a new `Uint8Array` backed by a fresh `ArrayBuffer`. The Rust-side
///    `Bytes` buffer is freed when the `VideoFrame` is dropped. This is the
///    simplest correct approach — no lifetime coupling between Rust and JS.
///
/// 2. **SharedArrayBuffer** (future optimization): If `crossOriginIsolated` is
///    available, the WASM linear memory can be exposed as a `SharedArrayBuffer`,
///    allowing JS to read frame data in-place without copying. This requires
///    careful synchronization (the Rust side must not free or rewrite the buffer
///    while JS is reading). This is an opt-in optimization, not the default.
///
/// 3. **Transfer** (not viable): `ArrayBuffer.transfer()` moves ownership to JS,
///    but WASM linear memory cannot be transferred — it's a shared resource.
///    Only independent allocations outside linear memory (via `wasm-bindgen`'s
///    `Uint8Array::new_with_length`) can be transferred.
///
/// The copy-out model was chosen because:
/// - Correctness: no lifetime/ownership bugs possible
/// - Simplicity: works with standard `wasm-bindgen` without special headers
/// - Performance: for 1080p YUV420 (~3MB), copy overhead is <1ms on modern hardware
/// - Upgrade path: can switch to SharedArrayBuffer later without API changes
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Pixel format.
    pub pixel_format: PixelFormat,
    /// Color space metadata (`None` when the source did not signal color info).
    pub color_space: Option<ColorSpace>,
    /// Presentation timestamp.
    pub pts: Timestamp,
    /// Contiguous pixel data buffer containing all planes.
    pub data: Bytes,
    /// Layout descriptor for each plane, indexing into `data`.
    pub planes: Vec<PlaneLayout>,
}

/// Errors returned when constructing a `VideoFrame` with invalid plane layouts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VideoFrameError {
    #[error("plane {index} extends beyond buffer: offset {offset} + size {size} > buffer length {buffer_len}")]
    PlaneOutOfBounds {
        index: usize,
        offset: usize,
        size: usize,
        buffer_len: usize,
    },
    #[error("plane {index} stride {stride} is less than plane width {width}")]
    StrideTooSmall {
        index: usize,
        stride: usize,
        width: u32,
    },
}

impl VideoFrame {
    /// Creates a new `VideoFrame`, validating that all plane layouts fit within
    /// the data buffer.
    pub fn new(
        width: u32,
        height: u32,
        pixel_format: PixelFormat,
        color_space: Option<ColorSpace>,
        pts: Timestamp,
        data: Bytes,
        planes: Vec<PlaneLayout>,
    ) -> Result<Self, VideoFrameError> {
        for (i, plane) in planes.iter().enumerate() {
            if (plane.stride) < plane.width as usize {
                return Err(VideoFrameError::StrideTooSmall {
                    index: i,
                    stride: plane.stride,
                    width: plane.width,
                });
            }
            let end = plane.offset.saturating_add(plane.size());
            if end > data.len() {
                return Err(VideoFrameError::PlaneOutOfBounds {
                    index: i,
                    offset: plane.offset,
                    size: plane.size(),
                    buffer_len: data.len(),
                });
            }
        }
        Ok(Self {
            width,
            height,
            pixel_format,
            color_space,
            pts,
            data,
            planes,
        })
    }

    /// Returns the raw bytes for the given plane index.
    pub fn plane_data(&self, index: usize) -> Option<&[u8]> {
        self.planes
            .get(index)
            .map(|layout| &self.data[layout.offset..layout.offset + layout.size()])
    }

    /// Returns the contiguous frame buffer for WASM export.
    ///
    /// On the WASM target, `wasm-bindgen` copies this slice into a JS
    /// `Uint8Array`. The caller receives an independent copy — the Rust-side
    /// `VideoFrame` can be safely dropped afterward.
    ///
    /// # Lifetime semantics
    ///
    /// The returned slice borrows `self.data`. It is valid for the lifetime
    /// of this `VideoFrame`. Once the frame is dropped, the underlying
    /// `Bytes` buffer is freed. On the JS side, the `Uint8Array` returned
    /// by `wasm-bindgen` is an independent copy and remains valid.
    ///
    /// Plane offsets and strides should be communicated separately (e.g.,
    /// via a JSON metadata return) so JS knows how to interpret the buffer.
    pub fn as_wasm_buffer(&self) -> &[u8] {
        &self.data
    }
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
}
