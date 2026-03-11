use bytes::Bytes;
use splica_core::media::{AudioFrame, ChannelLayout, SampleFormat};
use splica_core::AudioFilter;
use splica_core::Timestamp;
use splica_filter::VolumeFilter;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_s16_frame(samples: &[i16]) -> AudioFrame {
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    AudioFrame {
        sample_rate: 48000,
        channel_layout: ChannelLayout::Mono,
        sample_format: SampleFormat::S16,
        sample_count: samples.len() as u32,
        pts: Timestamp::new(0, 48000).unwrap(),
        data: vec![Bytes::from(data)],
    }
}

fn make_s32_frame(samples: &[i32]) -> AudioFrame {
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    AudioFrame {
        sample_rate: 48000,
        channel_layout: ChannelLayout::Mono,
        sample_format: SampleFormat::S32,
        sample_count: samples.len() as u32,
        pts: Timestamp::new(0, 48000).unwrap(),
        data: vec![Bytes::from(data)],
    }
}

fn make_f32_frame(samples: &[f32]) -> AudioFrame {
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    AudioFrame {
        sample_rate: 48000,
        channel_layout: ChannelLayout::Mono,
        sample_format: SampleFormat::F32,
        sample_count: samples.len() as u32,
        pts: Timestamp::new(0, 48000).unwrap(),
        data: vec![Bytes::from(data)],
    }
}

fn make_f32_planar_stereo_frame(left: &[f32], right: &[f32]) -> AudioFrame {
    let left_data: Vec<u8> = left.iter().flat_map(|s| s.to_le_bytes()).collect();
    let right_data: Vec<u8> = right.iter().flat_map(|s| s.to_le_bytes()).collect();
    AudioFrame {
        sample_rate: 48000,
        channel_layout: ChannelLayout::Stereo,
        sample_format: SampleFormat::F32Planar,
        sample_count: left.len() as u32,
        pts: Timestamp::new(0, 48000).unwrap(),
        data: vec![Bytes::from(left_data), Bytes::from(right_data)],
    }
}

fn read_s16_samples(frame: &AudioFrame) -> Vec<i16> {
    frame.data[0]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

fn read_s32_samples(frame: &AudioFrame) -> Vec<i32> {
    frame.data[0]
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn read_f32_samples(frame: &AudioFrame) -> Vec<f32> {
    frame.data[0]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn read_f32_plane(frame: &AudioFrame, plane: usize) -> Vec<f32> {
    frame.data[plane]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn test_that_new_rejects_negative_gain() {
    let err = VolumeFilter::new(-1.0).unwrap_err();
    assert!(err.to_string().contains("non-negative"));
}

#[test]
fn test_that_new_rejects_nan_gain() {
    let err = VolumeFilter::new(f32::NAN).unwrap_err();
    assert!(err.to_string().contains("finite"));
}

#[test]
fn test_that_new_rejects_infinite_gain() {
    let err = VolumeFilter::new(f32::INFINITY).unwrap_err();
    assert!(err.to_string().contains("finite"));
}

#[test]
fn test_that_new_accepts_zero_gain() {
    assert!(VolumeFilter::new(0.0).is_ok());
}

#[test]
fn test_that_from_db_zero_is_unity() {
    let filter = VolumeFilter::from_db(0.0).unwrap();
    let frame = make_f32_frame(&[0.5]);

    let mut filter = filter;
    let out = filter.process(frame).unwrap();
    let samples = read_f32_samples(&out);

    assert!((samples[0] - 0.5).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// Unity gain (no-op)
// ---------------------------------------------------------------------------

#[test]
fn test_that_unity_gain_returns_frame_unchanged() {
    let frame = make_s16_frame(&[1000, -2000, 3000]);
    let mut filter = VolumeFilter::new(1.0).unwrap();

    let out = filter.process(frame).unwrap();
    let samples = read_s16_samples(&out);

    assert_eq!(samples, vec![1000, -2000, 3000]);
}

// ---------------------------------------------------------------------------
// S16 format
// ---------------------------------------------------------------------------

#[test]
fn test_that_s16_half_gain_halves_amplitude() {
    let frame = make_s16_frame(&[10000, -10000]);
    let mut filter = VolumeFilter::new(0.5).unwrap();

    let out = filter.process(frame).unwrap();
    let samples = read_s16_samples(&out);

    assert_eq!(samples, vec![5000, -5000]);
}

#[test]
fn test_that_s16_saturates_on_overflow() {
    let frame = make_s16_frame(&[i16::MAX, i16::MIN]);
    let mut filter = VolumeFilter::new(2.0).unwrap();

    let out = filter.process(frame).unwrap();
    let samples = read_s16_samples(&out);

    assert_eq!(samples, vec![i16::MAX, i16::MIN]);
}

#[test]
fn test_that_s16_zero_gain_produces_silence() {
    let frame = make_s16_frame(&[10000, -20000, 30000]);
    let mut filter = VolumeFilter::new(0.0).unwrap();

    let out = filter.process(frame).unwrap();
    let samples = read_s16_samples(&out);

    assert_eq!(samples, vec![0, 0, 0]);
}

// ---------------------------------------------------------------------------
// S32 format
// ---------------------------------------------------------------------------

#[test]
fn test_that_s32_half_gain_halves_amplitude() {
    let frame = make_s32_frame(&[100_000, -100_000]);
    let mut filter = VolumeFilter::new(0.5).unwrap();

    let out = filter.process(frame).unwrap();
    let samples = read_s32_samples(&out);

    assert_eq!(samples, vec![50_000, -50_000]);
}

#[test]
fn test_that_s32_saturates_on_overflow() {
    let frame = make_s32_frame(&[i32::MAX, i32::MIN]);
    let mut filter = VolumeFilter::new(2.0).unwrap();

    let out = filter.process(frame).unwrap();
    let samples = read_s32_samples(&out);

    assert_eq!(samples, vec![i32::MAX, i32::MIN]);
}

// ---------------------------------------------------------------------------
// F32 interleaved
// ---------------------------------------------------------------------------

#[test]
fn test_that_f32_half_gain_halves_amplitude() {
    let frame = make_f32_frame(&[0.8, -0.6]);
    let mut filter = VolumeFilter::new(0.5).unwrap();

    let out = filter.process(frame).unwrap();
    let samples = read_f32_samples(&out);

    assert!((samples[0] - 0.4).abs() < 1e-6);
    assert!((samples[1] - (-0.3)).abs() < 1e-6);
}

#[test]
fn test_that_f32_clamps_to_unit_range() {
    let frame = make_f32_frame(&[0.8, -0.9]);
    let mut filter = VolumeFilter::new(2.0).unwrap();

    let out = filter.process(frame).unwrap();
    let samples = read_f32_samples(&out);

    assert_eq!(samples, vec![1.0, -1.0]);
}

// ---------------------------------------------------------------------------
// F32 planar (stereo)
// ---------------------------------------------------------------------------

#[test]
fn test_that_f32_planar_applies_gain_to_all_channels() {
    let frame = make_f32_planar_stereo_frame(&[0.4, 0.6], &[-0.2, -0.8]);
    let mut filter = VolumeFilter::new(0.5).unwrap();

    let out = filter.process(frame).unwrap();
    let left = read_f32_plane(&out, 0);
    let right = read_f32_plane(&out, 1);

    assert!((left[0] - 0.2).abs() < 1e-6);
    assert!((left[1] - 0.3).abs() < 1e-6);
    assert!((right[0] - (-0.1)).abs() < 1e-6);
    assert!((right[1] - (-0.4)).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// Metadata preservation
// ---------------------------------------------------------------------------

#[test]
fn test_that_volume_preserves_frame_metadata() {
    let frame = AudioFrame {
        sample_rate: 44100,
        channel_layout: ChannelLayout::Stereo,
        sample_format: SampleFormat::F32,
        sample_count: 2,
        pts: Timestamp::new(1234, 48000).unwrap(),
        data: vec![Bytes::from(vec![0u8; 16])],
    };
    let mut filter = VolumeFilter::new(0.5).unwrap();

    let out = filter.process(frame).unwrap();

    assert_eq!(out.sample_rate, 44100);
    assert_eq!(out.channel_layout, ChannelLayout::Stereo);
    assert_eq!(out.sample_format, SampleFormat::F32);
    assert_eq!(out.sample_count, 2);
    assert_eq!(out.pts, Timestamp::new(1234, 48000).unwrap());
}

// ---------------------------------------------------------------------------
// dB conversion
// ---------------------------------------------------------------------------

#[test]
fn test_that_from_db_minus_6_approximately_halves() {
    let filter = VolumeFilter::from_db(-6.0206).unwrap();
    let frame = make_f32_frame(&[1.0]);

    let mut filter = filter;
    let out = filter.process(frame).unwrap();
    let samples = read_f32_samples(&out);

    assert!((samples[0] - 0.5).abs() < 0.01);
}
