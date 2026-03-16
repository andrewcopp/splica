//! Opus encoder implementation using libopus (FFI).
//!
//! Encodes raw PCM audio frames into Opus packets. Uses the reference
//! libopus encoder via the `opus` crate. Behind the `codec-opus` feature flag.

use std::any::Any;

use bytes::Bytes;
use splica_core::error::EncodeError;
use splica_core::media::{AudioFrame, ChannelLayout, Packet, SampleFormat, TrackIndex};
use splica_core::AudioEncoder;

use opus::Channels;

use crate::error::CodecError;

/// Opus encoder configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusEncoderConfig {
    /// Target bitrate in bits per second.
    pub bitrate_bps: u32,
    /// Sample rate in Hz (must be 8000, 12000, 16000, 24000, or 48000).
    pub sample_rate: u32,
    /// Channel layout.
    pub channel_layout: ChannelLayout,
}

/// Opus encoder wrapping libopus.
///
/// Accepts interleaved F32 or S16 `AudioFrame`s and produces Opus packets.
/// The Opus frame size is 960 samples at 48000 Hz (20ms).
pub struct OpusEncoder {
    inner: opus::Encoder,
    config: OpusEncoderConfig,
    track_index: TrackIndex,
    pending_packet: Option<Packet>,
    flushing: bool,
}

/// Builder for creating an `OpusEncoder` with specific settings.
#[must_use]
pub struct OpusEncoderBuilder {
    bitrate_bps: u32,
    sample_rate: u32,
    channel_layout: ChannelLayout,
    track_index: TrackIndex,
}

impl OpusEncoderBuilder {
    /// Creates a new encoder builder with default settings.
    ///
    /// Default: 128 kbps, 48000 Hz, Stereo.
    pub fn new() -> Self {
        Self {
            bitrate_bps: 128_000,
            sample_rate: 48000,
            channel_layout: ChannelLayout::Stereo,
            track_index: TrackIndex(0),
        }
    }

    /// Sets the target bitrate in bits per second.
    pub fn bitrate(mut self, bps: u32) -> Self {
        self.bitrate_bps = bps;
        self
    }

    /// Sets the sample rate in Hz.
    ///
    /// Opus supports: 8000, 12000, 16000, 24000, 48000.
    pub fn sample_rate(mut self, rate: u32) -> Self {
        self.sample_rate = rate;
        self
    }

    /// Sets the channel layout.
    pub fn channel_layout(mut self, layout: ChannelLayout) -> Self {
        self.channel_layout = layout;
        self
    }

    /// Sets the track index for output packets.
    pub fn track_index(mut self, index: TrackIndex) -> Self {
        self.track_index = index;
        self
    }

    /// Builds the Opus encoder.
    pub fn build(self) -> Result<OpusEncoder, CodecError> {
        let channels = match self.channel_layout {
            ChannelLayout::Mono => Channels::Mono,
            ChannelLayout::Stereo => Channels::Stereo,
            _ => {
                return Err(CodecError::InvalidConfig {
                    message: format!(
                        "Opus encoder supports Mono or Stereo, got {:?}",
                        self.channel_layout
                    ),
                });
            }
        };

        let mut inner = opus::Encoder::new(self.sample_rate, channels, opus::Application::Audio)
            .map_err(|e| CodecError::EncoderError {
                message: format!("failed to create Opus encoder: {e}"),
            })?;

        inner
            .set_bitrate(opus::Bitrate::Bits(self.bitrate_bps as i32))
            .map_err(|e| CodecError::EncoderError {
                message: format!("failed to set Opus bitrate: {e}"),
            })?;

        Ok(OpusEncoder {
            inner,
            config: OpusEncoderConfig {
                bitrate_bps: self.bitrate_bps,
                sample_rate: self.sample_rate,
                channel_layout: self.channel_layout,
            },
            track_index: self.track_index,
            pending_packet: None,
            flushing: false,
        })
    }
}

impl Default for OpusEncoderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl OpusEncoder {
    /// Creates a new Opus encoder with default settings (128 kbps, 48000 Hz, Stereo).
    pub fn new() -> Result<Self, CodecError> {
        OpusEncoderBuilder::new().build()
    }

    /// Returns a builder for configuring the encoder.
    pub fn builder() -> OpusEncoderBuilder {
        OpusEncoderBuilder::new()
    }

    /// Returns the encoder configuration.
    pub fn encoder_config(&self) -> &OpusEncoderConfig {
        &self.config
    }

    /// Converts interleaved F32 byte data to f32 slice.
    fn bytes_to_f32(data: &[u8]) -> Vec<f32> {
        let count = data.len() / 4;
        let mut samples = Vec::with_capacity(count);
        for i in 0..count {
            let bytes = [
                data[i * 4],
                data[i * 4 + 1],
                data[i * 4 + 2],
                data[i * 4 + 3],
            ];
            samples.push(f32::from_le_bytes(bytes));
        }
        samples
    }
}

impl AudioEncoder for OpusEncoder {
    fn send_frame(&mut self, frame: Option<&AudioFrame>) -> Result<(), EncodeError> {
        match frame {
            Some(audio_frame) => {
                if audio_frame.data.is_empty() {
                    return Err(EncodeError::InvalidFrame {
                        message: "audio frame has no data".to_string(),
                    });
                }

                // Encode using float API
                let samples = match audio_frame.sample_format {
                    SampleFormat::F32 => Self::bytes_to_f32(&audio_frame.data[0]),
                    SampleFormat::S16 => {
                        // Convert S16 to F32
                        let data = &audio_frame.data[0];
                        let count = data.len() / 2;
                        let mut floats = Vec::with_capacity(count);
                        for i in 0..count {
                            let bytes = [data[i * 2], data[i * 2 + 1]];
                            let sample = i16::from_le_bytes(bytes);
                            floats.push(sample as f32 / i16::MAX as f32);
                        }
                        floats
                    }
                    other => {
                        return Err(EncodeError::InvalidFrame {
                            message: format!(
                                "Opus encoder requires F32 or S16 input, got {other:?}"
                            ),
                        });
                    }
                };

                // Opus max packet size is ~4000 bytes for most configurations
                let mut output_buf = vec![0u8; 4096];
                let encoded_size =
                    self.inner
                        .encode_float(&samples, &mut output_buf)
                        .map_err(|e| CodecError::EncoderError {
                            message: format!("Opus encode error: {e}"),
                        })?;

                output_buf.truncate(encoded_size);
                let packet = Packet {
                    track_index: self.track_index,
                    pts: audio_frame.pts,
                    dts: audio_frame.pts,
                    is_keyframe: true, // Opus frames are independently decodable
                    data: Bytes::from(output_buf),
                };
                self.pending_packet = Some(packet);
            }
            None => {
                self.flushing = true;
            }
        }

        Ok(())
    }

    fn receive_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending_packet.take())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use splica_core::Timestamp;

    #[test]
    fn test_that_opus_encoder_creates_with_defaults() {
        let encoder = OpusEncoder::new();

        assert!(encoder.is_ok());
    }

    #[test]
    fn test_that_opus_encoder_config_is_accessible() {
        // GIVEN
        let encoder = OpusEncoderBuilder::new()
            .bitrate(96_000)
            .sample_rate(48000)
            .channel_layout(ChannelLayout::Mono)
            .build()
            .unwrap();

        // THEN
        let config = encoder.encoder_config();
        assert_eq!(config.bitrate_bps, 96_000);
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channel_layout, ChannelLayout::Mono);
    }

    #[test]
    fn test_that_opus_encoder_produces_packets_from_frames() {
        // GIVEN — an encoder and a synthetic audio frame
        let mut encoder = OpusEncoderBuilder::new()
            .sample_rate(48000)
            .channel_layout(ChannelLayout::Stereo)
            .build()
            .unwrap();

        // Opus standard frame size: 960 samples at 48000 Hz (20ms)
        let sample_count = 960u32;
        let channels = 2u32;
        let total_samples = sample_count * channels;
        let mut data = Vec::with_capacity(total_samples as usize * 4);
        for i in 0..total_samples {
            let t = i as f32 / (sample_count * channels) as f32;
            let sample = (t * std::f32::consts::TAU * 440.0).sin() * 0.5;
            data.extend_from_slice(&sample.to_le_bytes());
        }

        let frame = AudioFrame {
            sample_rate: 48000,
            channel_layout: ChannelLayout::Stereo,
            sample_format: SampleFormat::F32,
            sample_count,
            pts: Timestamp::new(0, 48000).unwrap(),
            data: vec![Bytes::from(data)],
        };

        // WHEN
        encoder.send_frame(Some(&frame)).unwrap();
        let packet = encoder.receive_packet().unwrap();

        // THEN — Opus always produces output immediately
        assert!(packet.is_some());
        let pkt = packet.unwrap();
        assert!(!pkt.data.is_empty());
    }

    #[test]
    fn test_that_opus_encoder_rejects_surround() {
        // GIVEN — Opus encoder only supports Mono/Stereo via the opus crate
        let result = OpusEncoderBuilder::new()
            .channel_layout(ChannelLayout::Surround5_1)
            .build();

        // THEN
        assert!(result.is_err());
    }

    #[test]
    fn test_that_opus_encoder_supports_downcasting() {
        // GIVEN
        let encoder = OpusEncoder::new().unwrap();
        let any_ref: &dyn Any = encoder.as_any();

        // THEN
        assert!(any_ref.downcast_ref::<OpusEncoder>().is_some());
    }

    #[test]
    fn test_that_opus_encoder_flush_completes() {
        // GIVEN
        let mut encoder = OpusEncoder::new().unwrap();

        // WHEN
        encoder.send_frame(None).unwrap();
        let packet = encoder.receive_packet().unwrap();

        // THEN
        assert!(packet.is_none());
    }
}
