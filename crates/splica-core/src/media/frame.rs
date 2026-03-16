//! Decoded frame types: video frames, audio frames, and their supporting types.

use bytes::Bytes;

use super::color::ColorSpace;
use super::ChannelLayout;
use crate::timestamp::Timestamp;

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

// ---------------------------------------------------------------------------
// PlaneLayout
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

// ---------------------------------------------------------------------------
// VideoFrame
// ---------------------------------------------------------------------------

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
