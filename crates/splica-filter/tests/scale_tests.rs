//! Integration tests for the scale filter.

use bytes::Bytes;
use splica_core::media::{ColorSpace, PixelFormat, PlaneLayout, VideoFrame};
use splica_core::timestamp::Timestamp;
use splica_core::VideoFilter;
use splica_filter::{AspectMode, Interpolation, ScaleFilter};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Builds a YUV420p VideoFrame with the given dimensions. Y plane is filled
/// with `y_val`, U/V planes with 128 (neutral chroma).
fn make_yuv420_frame(width: u32, height: u32, y_val: u8) -> VideoFrame {
    let y_size = (width * height) as usize;
    let uv_w = width / 2;
    let uv_h = height / 2;
    let uv_size = (uv_w * uv_h) as usize;

    let mut buf = vec![y_val; y_size];
    buf.extend(vec![128u8; uv_size]); // U
    buf.extend(vec![128u8; uv_size]); // V

    let planes = vec![
        PlaneLayout {
            offset: 0,
            stride: width as usize,
            width,
            height,
        },
        PlaneLayout {
            offset: y_size,
            stride: uv_w as usize,
            width: uv_w,
            height: uv_h,
        },
        PlaneLayout {
            offset: y_size + uv_size,
            stride: uv_w as usize,
            width: uv_w,
            height: uv_h,
        },
    ];

    VideoFrame::new(
        width,
        height,
        PixelFormat::Yuv420p,
        ColorSpace::BT709,
        Timestamp::new(0, 30).unwrap(),
        Bytes::from(buf),
        planes,
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_that_scale_filter_downscales_dimensions() {
    let frame = make_yuv420_frame(1920, 1080, 200);
    let mut filter = ScaleFilter::new(640, 480);

    let result = filter.process(frame).unwrap();

    assert_eq!(result.width, 640);
    assert_eq!(result.height, 480);
}

#[test]
fn test_that_scale_filter_upscales_dimensions() {
    let frame = make_yuv420_frame(320, 240, 100);
    let mut filter = ScaleFilter::new(1280, 720);

    let result = filter.process(frame).unwrap();

    assert_eq!(result.width, 1280);
    assert_eq!(result.height, 720);
}

#[test]
fn test_that_scale_filter_preserves_timestamp() {
    let frame = make_yuv420_frame(640, 480, 150);
    let mut filter = ScaleFilter::new(320, 240);

    let result = filter.process(frame).unwrap();

    assert_eq!(result.pts, Timestamp::new(0, 30).unwrap());
}

#[test]
fn test_that_scale_filter_noop_when_same_dimensions() {
    let frame = make_yuv420_frame(640, 480, 200);
    let mut filter = ScaleFilter::new(640, 480);

    let result = filter.process(frame).unwrap();

    assert_eq!(result.width, 640);
    assert_eq!(result.height, 480);
}

#[test]
fn test_that_nearest_neighbor_preserves_uniform_values() {
    let frame = make_yuv420_frame(100, 100, 200);
    let mut filter = ScaleFilter::new(50, 50).with_interpolation(Interpolation::NearestNeighbor);

    let result = filter.process(frame).unwrap();
    let y_data = result.plane_data(0).unwrap();

    // Uniform input => uniform output
    assert!(y_data.iter().all(|&v| v == 200));
}

#[test]
fn test_that_bilinear_preserves_uniform_values() {
    let frame = make_yuv420_frame(100, 100, 200);
    let mut filter = ScaleFilter::new(50, 50).with_interpolation(Interpolation::Bilinear);

    let result = filter.process(frame).unwrap();
    let y_data = result.plane_data(0).unwrap();

    // Bilinear of uniform value should be that same value
    assert!(y_data.iter().all(|&v| v == 200));
}

#[test]
fn test_that_fit_mode_adds_letterbox_bars() {
    // 16:9 source into a 4:3 target => pillarbox (black bars on sides)
    let frame = make_yuv420_frame(160, 90, 200);
    let mut filter = ScaleFilter::new(120, 120).with_aspect_mode(AspectMode::Fit);

    let result = filter.process(frame).unwrap();

    assert_eq!(result.width, 120);
    assert_eq!(result.height, 120);

    // Top rows should be black (Y=0) since content is centered vertically
    let y_data = result.plane_data(0).unwrap();
    let top_row = &y_data[..120];
    assert!(
        top_row.iter().all(|&v| v == 0),
        "top row should be black bars"
    );
}

#[test]
fn test_that_fill_mode_produces_correct_dimensions() {
    let frame = make_yuv420_frame(160, 90, 200);
    let mut filter = ScaleFilter::new(100, 100).with_aspect_mode(AspectMode::Fill);

    let result = filter.process(frame).unwrap();

    assert_eq!(result.width, 100);
    assert_eq!(result.height, 100);
}

#[test]
fn test_that_scale_filter_rejects_non_yuv420p() {
    // Build an RGBA frame
    let width = 4u32;
    let height = 4u32;
    let buf = vec![0u8; (width * height * 4) as usize];
    let planes = vec![PlaneLayout {
        offset: 0,
        stride: (width * 4) as usize,
        width: width * 4,
        height,
    }];
    let frame = VideoFrame::new(
        width,
        height,
        PixelFormat::Rgba,
        ColorSpace::BT709,
        Timestamp::new(0, 30).unwrap(),
        Bytes::from(buf),
        planes,
    )
    .unwrap();

    let mut filter = ScaleFilter::new(8, 8);
    let result = filter.process(frame);

    assert!(result.is_err());
}

#[test]
fn test_that_scale_filter_produces_valid_yuv420_planes() {
    let frame = make_yuv420_frame(640, 480, 128);
    let mut filter = ScaleFilter::new(320, 240);

    let result = filter.process(frame).unwrap();

    assert_eq!(result.planes.len(), 3);
    assert_eq!(result.planes[0].width, 320);
    assert_eq!(result.planes[0].height, 240);
    assert_eq!(result.planes[1].width, 160);
    assert_eq!(result.planes[1].height, 120);
    assert_eq!(result.planes[2].width, 160);
    assert_eq!(result.planes[2].height, 120);
}
