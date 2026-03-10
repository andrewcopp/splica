//! Scale (resize) filter for video frames.
//!
//! Supports nearest-neighbor and bilinear interpolation for YUV420p frames,
//! with configurable aspect ratio handling (stretch, fit/letterbox, fill/crop).

use bytes::Bytes;
use splica_core::error::FilterError;
use splica_core::media::{PixelFormat, PlaneLayout, VideoFrame};
use splica_core::VideoFilter;

/// Interpolation method for scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Fastest. Picks the nearest source pixel.
    NearestNeighbor,
    /// Smoother. Bilinear interpolation of 4 surrounding pixels.
    Bilinear,
}

/// How to handle aspect ratio differences between source and target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectMode {
    /// Stretch source to fill target dimensions exactly (may distort).
    Stretch,
    /// Scale to fit within target, adding black bars (letterbox/pillarbox).
    Fit,
    /// Scale to fill target, cropping excess (center crop).
    Fill,
}

/// Configuration for the scale filter.
#[derive(Debug, Clone)]
pub struct ScaleFilter {
    target_width: u32,
    target_height: u32,
    interpolation: Interpolation,
    aspect_mode: AspectMode,
}

impl ScaleFilter {
    pub fn new(target_width: u32, target_height: u32) -> Self {
        Self {
            target_width,
            target_height,
            interpolation: Interpolation::Bilinear,
            aspect_mode: AspectMode::Stretch,
        }
    }

    pub fn with_interpolation(mut self, interpolation: Interpolation) -> Self {
        self.interpolation = interpolation;
        self
    }

    pub fn with_aspect_mode(mut self, aspect_mode: AspectMode) -> Self {
        self.aspect_mode = aspect_mode;
        self
    }
}

/// Computed region within the target frame where scaled content is placed.
struct ScaleRegion {
    /// Offset in target where content starts.
    dst_x: u32,
    dst_y: u32,
    /// Size of the content region in the target.
    dst_w: u32,
    dst_h: u32,
    /// Region of the source to sample from.
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
}

fn compute_region(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32, mode: AspectMode) -> ScaleRegion {
    match mode {
        AspectMode::Stretch => ScaleRegion {
            dst_x: 0,
            dst_y: 0,
            dst_w,
            dst_h,
            src_x: 0,
            src_y: 0,
            src_w,
            src_h,
        },
        AspectMode::Fit => {
            let scale_x = dst_w as f64 / src_w as f64;
            let scale_y = dst_h as f64 / src_h as f64;
            let scale = scale_x.min(scale_y);
            let content_w = ((src_w as f64 * scale) as u32).min(dst_w);
            let content_h = ((src_h as f64 * scale) as u32).min(dst_h);
            // Round to even for YUV chroma alignment
            let content_w = content_w & !1;
            let content_h = content_h & !1;
            let offset_x = (dst_w - content_w) / 2;
            let offset_y = (dst_h - content_h) / 2;
            ScaleRegion {
                dst_x: offset_x,
                dst_y: offset_y,
                dst_w: content_w,
                dst_h: content_h,
                src_x: 0,
                src_y: 0,
                src_w,
                src_h,
            }
        }
        AspectMode::Fill => {
            let scale_x = dst_w as f64 / src_w as f64;
            let scale_y = dst_h as f64 / src_h as f64;
            let scale = scale_x.max(scale_y);
            let sample_w = ((dst_w as f64 / scale) as u32).min(src_w);
            let sample_h = ((dst_h as f64 / scale) as u32).min(src_h);
            // Round to even for YUV chroma alignment
            let sample_w = sample_w & !1;
            let sample_h = sample_h & !1;
            let crop_x = (src_w - sample_w) / 2;
            let crop_y = (src_h - sample_h) / 2;
            ScaleRegion {
                dst_x: 0,
                dst_y: 0,
                dst_w,
                dst_h,
                src_x: crop_x,
                src_y: crop_y,
                src_w: sample_w,
                src_h: sample_h,
            }
        }
    }
}

/// Parameters for scaling a single plane.
struct PlaneScaleParams {
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
}

/// Scale a single plane using nearest-neighbor interpolation.
fn scale_plane_nearest(src: &[u8], src_stride: usize, p: &PlaneScaleParams) -> Vec<u8> {
    let dst_stride = p.dst_w as usize;
    let mut dst = vec![0u8; dst_stride * p.dst_h as usize];

    for y in 0..p.dst_h {
        let sy = p.src_y + (y as u64 * p.src_h as u64 / p.dst_h as u64) as u32;
        let src_row = (sy as usize) * src_stride;
        let dst_row = (y as usize) * dst_stride;
        for x in 0..p.dst_w {
            let sx = p.src_x + (x as u64 * p.src_w as u64 / p.dst_w as u64) as u32;
            dst[dst_row + x as usize] = src[src_row + sx as usize];
        }
    }
    dst
}

/// Scale a single plane using bilinear interpolation.
fn scale_plane_bilinear(src: &[u8], src_stride: usize, p: &PlaneScaleParams) -> Vec<u8> {
    let dst_stride = p.dst_w as usize;
    let mut dst = vec![0u8; dst_stride * p.dst_h as usize];
    let max_sx = if p.src_w > 0 { p.src_w - 1 } else { 0 };
    let max_sy = if p.src_h > 0 { p.src_h - 1 } else { 0 };

    for y in 0..p.dst_h {
        // Map destination pixel center to source coordinates
        let src_yf = (y as f64 + 0.5) * p.src_h as f64 / p.dst_h as f64 - 0.5;
        let sy0 = (src_yf.floor() as i64).clamp(0, max_sy as i64) as u32 + p.src_y;
        let sy1 = (src_yf.ceil() as i64).clamp(0, max_sy as i64) as u32 + p.src_y;
        let fy = src_yf.fract().max(0.0);

        let dst_row = (y as usize) * dst_stride;
        let src_row0 = (sy0 as usize) * src_stride;
        let src_row1 = (sy1 as usize) * src_stride;

        for x in 0..p.dst_w {
            let src_xf = (x as f64 + 0.5) * p.src_w as f64 / p.dst_w as f64 - 0.5;
            let sx0 = (src_xf.floor() as i64).clamp(0, max_sx as i64) as u32 + p.src_x;
            let sx1 = (src_xf.ceil() as i64).clamp(0, max_sx as i64) as u32 + p.src_x;
            let fx = src_xf.fract().max(0.0);

            let p00 = src[src_row0 + sx0 as usize] as f64;
            let p10 = src[src_row0 + sx1 as usize] as f64;
            let p01 = src[src_row1 + sx0 as usize] as f64;
            let p11 = src[src_row1 + sx1 as usize] as f64;

            let top = p00 + (p10 - p00) * fx;
            let bot = p01 + (p11 - p01) * fx;
            let val = top + (bot - top) * fy;

            dst[dst_row + x as usize] = val.round().clamp(0.0, 255.0) as u8;
        }
    }
    dst
}

fn scale_plane(
    src: &[u8],
    src_stride: usize,
    region: &ScaleRegion,
    chroma: bool,
    interp: Interpolation,
) -> Vec<u8> {
    // For chroma planes in 4:2:0, dimensions are halved
    let params = if chroma {
        PlaneScaleParams {
            src_x: region.src_x / 2,
            src_y: region.src_y / 2,
            src_w: region.src_w / 2,
            src_h: region.src_h / 2,
            dst_w: region.dst_w / 2,
            dst_h: region.dst_h / 2,
        }
    } else {
        PlaneScaleParams {
            src_x: region.src_x,
            src_y: region.src_y,
            src_w: region.src_w,
            src_h: region.src_h,
            dst_w: region.dst_w,
            dst_h: region.dst_h,
        }
    };

    match interp {
        Interpolation::NearestNeighbor => scale_plane_nearest(src, src_stride, &params),
        Interpolation::Bilinear => scale_plane_bilinear(src, src_stride, &params),
    }
}

/// Build a YUV420p output frame with black background and scaled content placed
/// at the given region.
fn build_yuv420_frame(
    target_w: u32,
    target_h: u32,
    region: &ScaleRegion,
    y_scaled: &[u8],
    u_scaled: &[u8],
    v_scaled: &[u8],
) -> (Vec<u8>, Vec<PlaneLayout>) {
    let y_stride = target_w as usize;
    let y_size = y_stride * target_h as usize;
    let uv_w = target_w / 2;
    let uv_h = target_h / 2;
    let uv_stride = uv_w as usize;
    let uv_size = uv_stride * uv_h as usize;

    let mut buf = vec![0u8; y_size + 2 * uv_size];

    // Y plane: black = 16 (studio range) or 0 (full range). Use 0 for simplicity.
    // Fill the content region
    let y_dst = &mut buf[..y_size];
    for row in 0..region.dst_h {
        let dst_offset = ((region.dst_y + row) as usize) * y_stride + region.dst_x as usize;
        let src_offset = (row as usize) * region.dst_w as usize;
        y_dst[dst_offset..dst_offset + region.dst_w as usize]
            .copy_from_slice(&y_scaled[src_offset..src_offset + region.dst_w as usize]);
    }

    // U plane: neutral = 128
    let u_dst = &mut buf[y_size..y_size + uv_size];
    u_dst.fill(128); // neutral chroma for black bars
    let rdx = region.dst_x / 2;
    let rdy = region.dst_y / 2;
    let rdw = region.dst_w / 2;
    let rdh = region.dst_h / 2;
    for row in 0..rdh {
        let dst_offset = ((rdy + row) as usize) * uv_stride + rdx as usize;
        let src_offset = (row as usize) * rdw as usize;
        u_dst[dst_offset..dst_offset + rdw as usize]
            .copy_from_slice(&u_scaled[src_offset..src_offset + rdw as usize]);
    }

    // V plane: neutral = 128
    let v_dst = &mut buf[y_size + uv_size..];
    v_dst.fill(128);
    for row in 0..rdh {
        let dst_offset = ((rdy + row) as usize) * uv_stride + rdx as usize;
        let src_offset = (row as usize) * rdw as usize;
        v_dst[dst_offset..dst_offset + rdw as usize]
            .copy_from_slice(&v_scaled[src_offset..src_offset + rdw as usize]);
    }

    let planes = vec![
        PlaneLayout {
            offset: 0,
            stride: y_stride,
            width: target_w,
            height: target_h,
        },
        PlaneLayout {
            offset: y_size,
            stride: uv_stride,
            width: uv_w,
            height: uv_h,
        },
        PlaneLayout {
            offset: y_size + uv_size,
            stride: uv_stride,
            width: uv_w,
            height: uv_h,
        },
    ];

    (buf, planes)
}

impl VideoFilter for ScaleFilter {
    fn process(&mut self, frame: VideoFrame) -> Result<VideoFrame, FilterError> {
        if frame.pixel_format != PixelFormat::Yuv420p {
            return Err(FilterError::InvalidInput {
                message: format!(
                    "scale filter only supports YUV420p, got {:?}",
                    frame.pixel_format
                ),
            });
        }

        if frame.planes.len() < 3 {
            return Err(FilterError::InvalidInput {
                message: format!("expected 3 planes for YUV420p, got {}", frame.planes.len()),
            });
        }

        // Ensure target dimensions are even (YUV420p requirement)
        let target_w = self.target_width & !1;
        let target_h = self.target_height & !1;

        // No-op if dimensions match
        if frame.width == target_w && frame.height == target_h {
            return Ok(frame);
        }

        let region = compute_region(
            frame.width,
            frame.height,
            target_w,
            target_h,
            self.aspect_mode,
        );

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

        let y_scaled = scale_plane(
            y_data,
            frame.planes[0].stride,
            &region,
            false,
            self.interpolation,
        );
        let u_scaled = scale_plane(
            u_data,
            frame.planes[1].stride,
            &region,
            true,
            self.interpolation,
        );
        let v_scaled = scale_plane(
            v_data,
            frame.planes[2].stride,
            &region,
            true,
            self.interpolation,
        );

        let (buf, planes) =
            build_yuv420_frame(target_w, target_h, &region, &y_scaled, &u_scaled, &v_scaled);

        VideoFrame::new(
            target_w,
            target_h,
            PixelFormat::Yuv420p,
            frame.color_space,
            frame.pts,
            Bytes::from(buf),
            planes,
        )
        .map_err(|e| FilterError::Other(Box::new(e)))
    }
}
