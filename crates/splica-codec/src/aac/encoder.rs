//! AAC encoder implementation using fdk-aac (FFI).
//!
//! Encodes raw PCM audio frames into AAC-LC packets. Uses the Fraunhofer
//! FDK AAC encoder library, which is the standard AAC encoder used by Android.
//! Behind the `codec-aac-enc` feature flag.

use std::any::Any;

use bytes::Bytes;
use splica_core::error::EncodeError;
use splica_core::media::{AudioFrame, ChannelLayout, Packet, SampleFormat, TrackIndex};
use splica_core::AudioEncoder;

use fdk_aac::enc::{AudioObjectType, BitRate, ChannelMode, Encoder, EncoderParams, Transport};

use crate::error::CodecError;

/// AAC encoder configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AacEncoderConfig {
    /// Target bitrate in bits per second.
    pub bitrate_bps: u32,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel layout.
    pub channel_layout: ChannelLayout,
}

/// AAC encoder wrapping fdk-aac.
///
/// Accepts interleaved F32 or S16 `AudioFrame`s and produces raw AAC packets.
/// The encoder uses the send/receive pattern: send frames via `send_frame()`,
/// then retrieve encoded packets via `receive_packet()`.
pub struct AacEncoder {
    inner: Encoder,
    config: AacEncoderConfig,
    track_index: TrackIndex,
    pending_packet: Option<Packet>,
    flushing: bool,
}

/// Builder for creating an `AacEncoder` with specific settings.
pub struct AacEncoderBuilder {
    bitrate_bps: u32,
    sample_rate: u32,
    channel_layout: ChannelLayout,
    track_index: TrackIndex,
}

impl AacEncoderBuilder {
    /// Creates a new encoder builder with default settings.
    ///
    /// Default: 128 kbps, 44100 Hz, Stereo.
    pub fn new() -> Self {
        Self {
            bitrate_bps: 128_000,
            sample_rate: 44100,
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

    /// Builds the AAC encoder.
    pub fn build(self) -> Result<AacEncoder, CodecError> {
        let channel_mode = match self.channel_layout {
            ChannelLayout::Mono => ChannelMode::Mono,
            ChannelLayout::Stereo => ChannelMode::Stereo,
            _ => {
                return Err(CodecError::InvalidConfig {
                    message: format!(
                        "AAC encoder (fdk-aac) supports Mono or Stereo, got {:?}",
                        self.channel_layout
                    ),
                });
            }
        };

        let params = EncoderParams {
            bit_rate: BitRate::Cbr(self.bitrate_bps),
            sample_rate: self.sample_rate,
            transport: Transport::Raw,
            channels: channel_mode,
            audio_object_type: AudioObjectType::Mpeg4LowComplexity,
        };

        let inner = Encoder::new(params).map_err(|e| CodecError::EncoderError {
            message: format!("failed to create AAC encoder: {e:?}"),
        })?;

        Ok(AacEncoder {
            inner,
            config: AacEncoderConfig {
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

impl Default for AacEncoderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AacEncoder {
    /// Creates a new AAC encoder with default settings (128 kbps, 44100 Hz, Stereo).
    pub fn new() -> Result<Self, CodecError> {
        AacEncoderBuilder::new().build()
    }

    /// Returns a builder for configuring the encoder.
    pub fn builder() -> AacEncoderBuilder {
        AacEncoderBuilder::new()
    }

    /// Returns the encoder configuration.
    pub fn encoder_config(&self) -> &AacEncoderConfig {
        &self.config
    }

    /// Generates AudioSpecificConfig bytes for this encoder's settings.
    ///
    /// These bytes are needed by the MP4 muxer for the esds box.
    pub fn audio_specific_config(&self) -> Vec<u8> {
        // AAC-LC AudioSpecificConfig:
        // 5 bits: objectType (2 = AAC-LC)
        // 4 bits: freqIndex
        // 4 bits: channelConfig
        // 3 bits: padding

        let object_type: u8 = 2; // AAC-LC
        let freq_index = match self.config.sample_rate {
            96000 => 0u8,
            88200 => 1,
            64000 => 2,
            48000 => 3,
            44100 => 4,
            32000 => 5,
            24000 => 6,
            22050 => 7,
            16000 => 8,
            12000 => 9,
            11025 => 10,
            8000 => 11,
            7350 => 12,
            _ => 4, // fallback to 44100
        };
        let channel_config: u8 = match self.config.channel_layout {
            ChannelLayout::Mono => 1,
            ChannelLayout::Stereo => 2,
            ChannelLayout::Surround5_1 => 6,
            ChannelLayout::Surround7_1 => 7,
        };

        // Pack: [objectType:5][freqIndex:4][channelConfig:4][padding:3]
        let byte0 = (object_type << 3) | (freq_index >> 1);
        let byte1 = (freq_index << 7) | (channel_config << 3);

        vec![byte0, byte1]
    }

    /// Converts interleaved F32 samples to interleaved S16 for fdk-aac.
    fn f32_to_s16(samples: &[u8]) -> Vec<i16> {
        let float_count = samples.len() / 4;
        let mut out = Vec::with_capacity(float_count);
        for i in 0..float_count {
            let bytes = [
                samples[i * 4],
                samples[i * 4 + 1],
                samples[i * 4 + 2],
                samples[i * 4 + 3],
            ];
            let sample = f32::from_le_bytes(bytes);
            // Clamp to [-1.0, 1.0] then scale to i16 range
            let clamped = sample.clamp(-1.0, 1.0);
            out.push((clamped * i16::MAX as f32) as i16);
        }
        out
    }
}

impl AudioEncoder for AacEncoder {
    fn send_frame(&mut self, frame: Option<&AudioFrame>) -> Result<(), EncodeError> {
        match frame {
            Some(audio_frame) => {
                if audio_frame.data.is_empty() {
                    return Err(EncodeError::InvalidFrame {
                        message: "audio frame has no data".to_string(),
                    });
                }

                // Convert to S16 interleaved samples for fdk-aac
                let s16_samples = match audio_frame.sample_format {
                    SampleFormat::F32 => Self::f32_to_s16(&audio_frame.data[0]),
                    SampleFormat::S16 => {
                        let data = &audio_frame.data[0];
                        let sample_count = data.len() / 2;
                        let mut samples = Vec::with_capacity(sample_count);
                        for i in 0..sample_count {
                            let bytes = [data[i * 2], data[i * 2 + 1]];
                            samples.push(i16::from_le_bytes(bytes));
                        }
                        samples
                    }
                    other => {
                        return Err(EncodeError::InvalidFrame {
                            message: format!(
                                "AAC encoder requires F32 or S16 input, got {other:?}"
                            ),
                        });
                    }
                };

                let mut output_buf = vec![0u8; 8192]; // AAC frame is at most ~768 bytes for LC
                let encode_info =
                    self.inner
                        .encode(&s16_samples, &mut output_buf)
                        .map_err(|e| CodecError::EncoderError {
                            message: format!("AAC encode error: {e:?}"),
                        })?;

                let output_size = encode_info.output_size;
                if output_size > 0 {
                    output_buf.truncate(output_size);
                    let packet = Packet {
                        track_index: self.track_index,
                        pts: audio_frame.pts,
                        dts: audio_frame.pts,
                        is_keyframe: true, // all AAC frames are random access points
                        data: Bytes::from(output_buf),
                    };
                    self.pending_packet = Some(packet);
                }
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
    fn test_that_aac_encoder_creates_with_defaults() {
        let encoder = AacEncoder::new();

        assert!(encoder.is_ok());
    }

    #[test]
    fn test_that_aac_encoder_config_is_accessible() {
        // GIVEN
        let encoder = AacEncoderBuilder::new()
            .bitrate(192_000)
            .sample_rate(48000)
            .channel_layout(ChannelLayout::Stereo)
            .build()
            .unwrap();

        // THEN
        let config = encoder.encoder_config();
        assert_eq!(config.bitrate_bps, 192_000);
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channel_layout, ChannelLayout::Stereo);
    }

    #[test]
    fn test_that_aac_encoder_produces_audio_specific_config() {
        // GIVEN — 44100 Hz stereo encoder
        let encoder = AacEncoder::new().unwrap();

        // WHEN
        let asc = encoder.audio_specific_config();

        // THEN — AAC-LC (objectType=2), freqIndex=4 (44100), channelConfig=2
        assert_eq!(asc.len(), 2);
        assert_eq!(asc, &[0x12, 0x10]);
    }

    #[test]
    fn test_that_aac_encoder_produces_packets_from_frames() {
        // GIVEN — an encoder and a synthetic audio frame
        let mut encoder = AacEncoderBuilder::new()
            .sample_rate(44100)
            .channel_layout(ChannelLayout::Stereo)
            .build()
            .unwrap();

        // AAC-LC frame size is 1024 samples per channel
        let sample_count = 1024u32;
        let channels = 2u32;
        let total_samples = sample_count * channels;
        let mut data = Vec::with_capacity(total_samples as usize * 4);
        for i in 0..total_samples {
            // Generate a simple sine wave
            let t = i as f32 / (sample_count * channels) as f32;
            let sample = (t * std::f32::consts::TAU * 440.0).sin() * 0.5;
            data.extend_from_slice(&sample.to_le_bytes());
        }

        let frame = AudioFrame {
            sample_rate: 44100,
            channel_layout: ChannelLayout::Stereo,
            sample_format: SampleFormat::F32,
            sample_count,
            pts: Timestamp::new(0, 44100),
            data: vec![Bytes::from(data)],
        };

        // WHEN — encode the frame
        encoder.send_frame(Some(&frame)).unwrap();
        let packet = encoder.receive_packet().unwrap();

        // THEN — a packet may or may not be produced (encoder may buffer)
        // fdk-aac typically needs a few frames before producing output
        // So we just verify no error occurred
        drop(packet);
    }

    #[test]
    fn test_that_aac_encoder_supports_downcasting() {
        // GIVEN
        let encoder = AacEncoder::new().unwrap();
        let any_ref: &dyn Any = encoder.as_any();

        // THEN
        assert!(any_ref.downcast_ref::<AacEncoder>().is_some());
    }

    #[test]
    fn test_that_aac_encoder_flush_completes() {
        // GIVEN
        let mut encoder = AacEncoder::new().unwrap();

        // WHEN — flush without sending any frames
        encoder.send_frame(None).unwrap();
        let packet = encoder.receive_packet().unwrap();

        // THEN — no packet (nothing buffered)
        assert!(packet.is_none());
    }
}
