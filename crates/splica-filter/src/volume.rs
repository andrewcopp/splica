//! Volume (gain) filter for audio frames.
//!
//! Applies a scalar gain to all samples, with saturation for integer formats
//! and clamping for float formats.

use bytes::Bytes;
use splica_core::error::FilterError;
use splica_core::media::{AudioFrame, SampleFormat};
use splica_core::AudioFilter;

/// A stateless audio filter that scales sample amplitude by a gain factor.
///
/// A gain of `1.0` is unity (no change), `0.5` halves amplitude (≈ −6 dB),
/// and `2.0` doubles it (≈ +6 dB). A gain of `0.0` produces silence.
#[derive(Debug, Clone)]
pub struct VolumeFilter {
    gain: f32,
}

impl VolumeFilter {
    /// Creates a new `VolumeFilter` with the given linear gain factor.
    ///
    /// # Errors
    ///
    /// Returns `FilterError::InvalidInput` if `gain` is negative, NaN, or infinite.
    pub fn new(gain: f32) -> Result<Self, FilterError> {
        if gain.is_nan() || gain.is_infinite() || gain < 0.0 {
            return Err(FilterError::InvalidInput {
                message: format!("volume gain must be a finite non-negative number, got {gain}"),
            });
        }
        Ok(Self { gain })
    }

    /// Creates a `VolumeFilter` from a decibel value.
    ///
    /// `0 dB` = unity, `−6 dB` ≈ half amplitude, `+6 dB` ≈ double amplitude.
    ///
    /// # Errors
    ///
    /// Returns `FilterError::InvalidInput` if `db` is NaN or positive infinity.
    pub fn from_db(db: f32) -> Result<Self, FilterError> {
        if db.is_nan() || db == f32::INFINITY {
            return Err(FilterError::InvalidInput {
                message: format!("volume dB must be a finite number, got {db}"),
            });
        }
        let gain = 10f32.powf(db / 20.0);
        Self::new(gain)
    }
}

impl AudioFilter for VolumeFilter {
    fn process(&mut self, frame: AudioFrame) -> Result<AudioFrame, FilterError> {
        // Unity gain is a no-op.
        if (self.gain - 1.0).abs() < f32::EPSILON {
            return Ok(frame);
        }

        let data = match frame.sample_format {
            SampleFormat::S16 => apply_gain_s16(&frame.data, self.gain),
            SampleFormat::S32 => apply_gain_s32(&frame.data, self.gain),
            SampleFormat::F32 => apply_gain_f32(&frame.data, self.gain),
            SampleFormat::F32Planar => apply_gain_f32(&frame.data, self.gain),
        };

        Ok(AudioFrame {
            sample_rate: frame.sample_rate,
            channel_layout: frame.channel_layout,
            sample_format: frame.sample_format,
            sample_count: frame.sample_count,
            pts: frame.pts,
            data,
        })
    }
}

/// Apply gain to 16-bit signed integer samples with saturation.
fn apply_gain_s16(planes: &[Bytes], gain: f32) -> Vec<Bytes> {
    planes
        .iter()
        .map(|plane| {
            let mut out = Vec::with_capacity(plane.len());
            for chunk in plane.chunks_exact(2) {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                let scaled = (sample as f32 * gain).round();
                let clamped = scaled.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                out.extend_from_slice(&clamped.to_le_bytes());
            }
            Bytes::from(out)
        })
        .collect()
}

/// Apply gain to 32-bit signed integer samples with saturation.
fn apply_gain_s32(planes: &[Bytes], gain: f32) -> Vec<Bytes> {
    planes
        .iter()
        .map(|plane| {
            let mut out = Vec::with_capacity(plane.len());
            for chunk in plane.chunks_exact(4) {
                let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let scaled = (sample as f64 * gain as f64).round();
                let clamped = scaled.clamp(i32::MIN as f64, i32::MAX as f64) as i32;
                out.extend_from_slice(&clamped.to_le_bytes());
            }
            Bytes::from(out)
        })
        .collect()
}

/// Apply gain to 32-bit float samples (interleaved or planar) with clamping.
fn apply_gain_f32(planes: &[Bytes], gain: f32) -> Vec<Bytes> {
    planes
        .iter()
        .map(|plane| {
            let mut out = Vec::with_capacity(plane.len());
            for chunk in plane.chunks_exact(4) {
                let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let scaled = (sample * gain).clamp(-1.0, 1.0);
                out.extend_from_slice(&scaled.to_le_bytes());
            }
            Bytes::from(out)
        })
        .collect()
}
