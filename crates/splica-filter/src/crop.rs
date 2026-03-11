//! Crop filter for video frames.
//!
//! Extracts a rectangular sub-region from YUV420p frames. Crop coordinates
//! are rounded to even values for chroma alignment.

use bytes::Bytes;
use splica_core::error::FilterError;
use splica_core::media::{PixelFormat, PlaneLayout, VideoFrame};
use splica_core::VideoFilter;

/// Crop filter that extracts a rectangular region from each video frame.
///
/// Coordinates are snapped to even values to maintain YUV420p chroma alignment.
#[derive(Debug, Clone)]
pub struct CropFilter {
    /// Left offset (snapped to even).
    x: u32,
    /// Top offset (snapped to even).
    y: u32,
    /// Output width (snapped to even).
    width: u32,
    /// Output height (snapped to even).
    height: u32,
}

impl CropFilter {
    /// Creates a new crop filter.
    ///
    /// All values are snapped to even for YUV420p chroma alignment.
    /// Returns an error if width or height is zero after snapping.
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Result<Self, FilterError> {
        let width = width & !1;
        let height = height & !1;
        let x = x & !1;
        let y = y & !1;

        if width == 0 || height == 0 {
            return Err(FilterError::InvalidInput {
                message: format!(
                    "crop dimensions must be non-zero after even-alignment: {width}x{height}"
                ),
            });
        }

        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    /// Returns the crop region as (x, y, width, height).
    pub fn region(&self) -> (u32, u32, u32, u32) {
        (self.x, self.y, self.width, self.height)
    }
}

/// Copies a rectangular sub-region from a plane into a new contiguous buffer.
fn crop_plane(src: &[u8], src_stride: usize, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let dst_stride = w as usize;
    let mut dst = vec![0u8; dst_stride * h as usize];

    for row in 0..h as usize {
        let src_offset = (y as usize + row) * src_stride + x as usize;
        let dst_offset = row * dst_stride;
        dst[dst_offset..dst_offset + dst_stride]
            .copy_from_slice(&src[src_offset..src_offset + dst_stride]);
    }

    dst
}

impl VideoFilter for CropFilter {
    fn process(&mut self, frame: VideoFrame) -> Result<VideoFrame, FilterError> {
        if frame.pixel_format != PixelFormat::Yuv420p {
            return Err(FilterError::InvalidInput {
                message: format!(
                    "crop filter only supports YUV420p, got {:?}",
                    frame.pixel_format
                ),
            });
        }

        if frame.planes.len() < 3 {
            return Err(FilterError::InvalidInput {
                message: format!("expected 3 planes for YUV420p, got {}", frame.planes.len()),
            });
        }

        // Validate that the crop region fits within the source frame
        if self.x + self.width > frame.width || self.y + self.height > frame.height {
            return Err(FilterError::InvalidInput {
                message: format!(
                    "crop region {}x{}+{}+{} exceeds source frame {}x{}",
                    self.width, self.height, self.x, self.y, frame.width, frame.height
                ),
            });
        }

        // No-op if crop covers the entire frame
        if self.x == 0 && self.y == 0 && self.width == frame.width && self.height == frame.height {
            return Ok(frame);
        }

        let y_data = frame
            .plane_data(0)
            .ok_or_else(|| FilterError::InvalidInput {
                message: "missing Y plane".to_string(),
            })?;
        let u_data = frame
            .plane_data(1)
            .ok_or_else(|| FilterError::InvalidInput {
                message: "missing U plane".to_string(),
            })?;
        let v_data = frame
            .plane_data(2)
            .ok_or_else(|| FilterError::InvalidInput {
                message: "missing V plane".to_string(),
            })?;

        // Crop Y plane (full resolution)
        let y_cropped = crop_plane(
            y_data,
            frame.planes[0].stride,
            self.x,
            self.y,
            self.width,
            self.height,
        );

        // Crop U and V planes (half resolution for YUV420p)
        let cx = self.x / 2;
        let cy = self.y / 2;
        let cw = self.width / 2;
        let ch = self.height / 2;

        let u_cropped = crop_plane(u_data, frame.planes[1].stride, cx, cy, cw, ch);
        let v_cropped = crop_plane(v_data, frame.planes[2].stride, cx, cy, cw, ch);

        // Build output buffer
        let y_size = self.width as usize * self.height as usize;
        let uv_size = cw as usize * ch as usize;
        let mut buf = Vec::with_capacity(y_size + 2 * uv_size);
        buf.extend_from_slice(&y_cropped);
        buf.extend_from_slice(&u_cropped);
        buf.extend_from_slice(&v_cropped);

        let planes = vec![
            PlaneLayout {
                offset: 0,
                stride: self.width as usize,
                width: self.width,
                height: self.height,
            },
            PlaneLayout {
                offset: y_size,
                stride: cw as usize,
                width: cw,
                height: ch,
            },
            PlaneLayout {
                offset: y_size + uv_size,
                stride: cw as usize,
                width: cw,
                height: ch,
            },
        ];

        VideoFrame::new(
            self.width,
            self.height,
            PixelFormat::Yuv420p,
            frame.color_space,
            frame.pts,
            Bytes::from(buf),
            planes,
        )
        .map_err(|e| FilterError::Other(Box::new(e)))
    }
}
