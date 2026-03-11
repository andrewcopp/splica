//! Integration tests for the crop filter.

use bytes::Bytes;
use splica_core::media::{ColorSpace, PixelFormat, PlaneLayout, VideoFrame};
use splica_core::timestamp::Timestamp;
use splica_core::VideoFilter;
use splica_filter::CropFilter;

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
        Some(ColorSpace::BT709),
        Timestamp::new(0, 30).unwrap(),
        Bytes::from(buf),
        planes,
    )
    .unwrap()
}

/// Builds a YUV420p frame with a gradient Y plane (value = x + y * width).
fn make_gradient_frame(width: u32, height: u32) -> VideoFrame {
    let y_size = (width * height) as usize;
    let uv_w = width / 2;
    let uv_h = height / 2;
    let uv_size = (uv_w * uv_h) as usize;

    let mut buf = Vec::with_capacity(y_size + 2 * uv_size);
    for y in 0..height {
        for x in 0..width {
            buf.push(((x + y * width) % 256) as u8);
        }
    }
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
        Some(ColorSpace::BT709),
        Timestamp::new(42, 30).unwrap(),
        Bytes::from(buf),
        planes,
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_that_crop_filter_produces_correct_dimensions() {
    let frame = make_yuv420_frame(1920, 1080, 200);
    let mut filter = CropFilter::new(420, 0, 1080, 1080).unwrap();

    let result = filter.process(frame).unwrap();

    assert_eq!(result.width, 1080);
    assert_eq!(result.height, 1080);
}

#[test]
fn test_that_crop_filter_preserves_timestamp() {
    let frame = make_gradient_frame(640, 480);
    let mut filter = CropFilter::new(0, 0, 320, 240).unwrap();

    let result = filter.process(frame).unwrap();

    assert_eq!(result.pts, Timestamp::new(42, 30).unwrap());
}

#[test]
fn test_that_crop_filter_preserves_color_space() {
    let frame = make_yuv420_frame(640, 480, 128);
    let mut filter = CropFilter::new(0, 0, 320, 240).unwrap();

    let result = filter.process(frame).unwrap();

    assert_eq!(result.color_space, Some(ColorSpace::BT709));
}

#[test]
fn test_that_crop_filter_rejects_non_yuv420p() {
    let frame = VideoFrame::new(
        640,
        480,
        PixelFormat::Yuv422p,
        None,
        Timestamp::new(0, 30).unwrap(),
        Bytes::from(vec![0u8; 640 * 480 * 2]),
        vec![
            PlaneLayout {
                offset: 0,
                stride: 640,
                width: 640,
                height: 480,
            },
            PlaneLayout {
                offset: 640 * 480,
                stride: 320,
                width: 320,
                height: 480,
            },
            PlaneLayout {
                offset: 640 * 480 + 320 * 480,
                stride: 320,
                width: 320,
                height: 480,
            },
        ],
    )
    .unwrap();

    let mut filter = CropFilter::new(0, 0, 320, 240).unwrap();
    let result = filter.process(frame);

    assert!(result.is_err());
}

#[test]
fn test_that_crop_filter_rejects_out_of_bounds_region() {
    let frame = make_yuv420_frame(640, 480, 128);
    let mut filter = CropFilter::new(400, 0, 320, 240).unwrap();

    let result = filter.process(frame);

    assert!(result.is_err());
}

#[test]
fn test_that_crop_filter_rejects_zero_width() {
    let result = CropFilter::new(0, 0, 1, 480);

    assert!(result.is_err());
}

#[test]
fn test_that_crop_filter_rejects_zero_height() {
    let result = CropFilter::new(0, 0, 640, 1);

    assert!(result.is_err());
}

#[test]
fn test_that_crop_filter_noop_when_full_frame() {
    let frame = make_yuv420_frame(640, 480, 200);
    let original_data_len = frame.data.len();
    let mut filter = CropFilter::new(0, 0, 640, 480).unwrap();

    let result = filter.process(frame).unwrap();

    assert_eq!(result.width, 640);
    assert_eq!(result.height, 480);
    assert_eq!(result.data.len(), original_data_len);
}

#[test]
fn test_that_crop_filter_produces_valid_yuv420_planes() {
    let frame = make_yuv420_frame(640, 480, 200);
    let mut filter = CropFilter::new(100, 100, 320, 240).unwrap();

    let result = filter.process(frame).unwrap();

    assert_eq!(result.planes.len(), 3);
    // Y plane
    assert_eq!(result.planes[0].width, 320);
    assert_eq!(result.planes[0].height, 240);
    assert_eq!(result.planes[0].stride, 320);
    // U plane
    assert_eq!(result.planes[1].width, 160);
    assert_eq!(result.planes[1].height, 120);
    assert_eq!(result.planes[1].stride, 160);
    // V plane
    assert_eq!(result.planes[2].width, 160);
    assert_eq!(result.planes[2].height, 120);
    assert_eq!(result.planes[2].stride, 160);
}

#[test]
fn test_that_crop_filter_extracts_correct_pixel_data() {
    let frame = make_gradient_frame(16, 16);
    // Crop a 4x4 region starting at (4, 4)
    let mut filter = CropFilter::new(4, 4, 4, 4).unwrap();

    let result = filter.process(frame).unwrap();

    let y_data = result.plane_data(0).unwrap();
    // First pixel of cropped region should be the value at (4, 4) in the original
    // gradient: (4 + 4 * 16) % 256 = 68
    assert_eq!(y_data[0], 68);
    // Second pixel: (5 + 4 * 16) % 256 = 69
    assert_eq!(y_data[1], 69);
    // First pixel of second row: (4 + 5 * 16) % 256 = 84
    assert_eq!(y_data[4], 84);
}

#[test]
fn test_that_crop_filter_snaps_odd_coordinates_to_even() {
    let filter = CropFilter::new(3, 5, 101, 101).unwrap();
    let (x, y, w, h) = filter.region();

    assert_eq!(x, 2);
    assert_eq!(y, 4);
    assert_eq!(w, 100);
    assert_eq!(h, 100);
}

#[test]
fn test_that_crop_filter_preserves_uniform_y_values() {
    let frame = make_yuv420_frame(640, 480, 200);
    let mut filter = CropFilter::new(100, 100, 200, 200).unwrap();

    let result = filter.process(frame).unwrap();

    let y_data = result.plane_data(0).unwrap();
    assert!(y_data.iter().all(|&v| v == 200));
}
