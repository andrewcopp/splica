//! Opus decoder implementation using libopus (FFI).
//!
//! Decodes Opus packets into raw PCM audio frames. Uses the reference
//! libopus decoder via the `opus` crate. Behind the `codec-opus` feature flag.

use std::any::Any;

use bytes::Bytes;
use splica_core::error::DecodeError;
use splica_core::media::{AudioFrame, ChannelLayout, Packet, SampleFormat};
use splica_core::AudioDecoder;

use opus::Channels;

use crate::error::CodecError;

/// Opus decoder configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusDecoderConfig {
    /// Sample rate in Hz (must be 8000, 12000, 16000, 24000, or 48000).
    pub sample_rate: u32,
    /// Channel layout.
    pub channel_layout: ChannelLayout,
}

/// Opus decoder wrapping libopus.
///
/// Accepts Opus packets and produces interleaved F32 `AudioFrame`s.
/// The frame size is determined from each packet's TOC byte via libopus,
/// rather than hardcoding a fixed value.
pub struct OpusDecoder {
    inner: opus::Decoder,
    config: OpusDecoderConfig,
    pending_frame: Option<AudioFrame>,
    flushing: bool,
}

impl OpusDecoder {
    /// Creates a new Opus decoder.
    ///
    /// The `sample_rate` and `channel_layout` must match the Opus stream
    /// parameters (typically from the container's track metadata).
    pub fn new(sample_rate: u32, channel_layout: ChannelLayout) -> Result<Self, CodecError> {
        let channels = match channel_layout {
            ChannelLayout::Mono => Channels::Mono,
            ChannelLayout::Stereo => Channels::Stereo,
            _ => {
                return Err(CodecError::InvalidConfig {
                    message: format!(
                        "Opus decoder supports Mono or Stereo, got {:?}",
                        channel_layout
                    ),
                });
            }
        };

        let inner =
            opus::Decoder::new(sample_rate, channels).map_err(|e| CodecError::DecoderError {
                message: format!("failed to create Opus decoder: {e}"),
            })?;

        Ok(Self {
            inner,
            config: OpusDecoderConfig {
                sample_rate,
                channel_layout,
            },
            pending_frame: None,
            flushing: false,
        })
    }

    /// Returns the decoder configuration.
    pub fn decoder_config(&self) -> &OpusDecoderConfig {
        &self.config
    }
}

impl AudioDecoder for OpusDecoder {
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError> {
        match packet {
            Some(pkt) => {
                let channel_count = self.config.channel_layout.channel_count() as usize;

                // Use libopus to determine the number of samples per channel
                // from the packet's TOC byte, rather than hardcoding 960.
                let nb_samples =
                    self.inner
                        .get_nb_samples(&pkt.data)
                        .map_err(|e| CodecError::DecoderError {
                            message: format!("failed to parse Opus packet header: {e}"),
                        })?;

                // Allocate output buffer: samples_per_channel * channels
                let total_samples = nb_samples * channel_count;
                let mut output = vec![0.0f32; total_samples];

                let decoded_samples = self
                    .inner
                    .decode_float(&pkt.data, &mut output, false)
                    .map_err(|e| CodecError::DecoderError {
                        message: format!("Opus decode error: {e}"),
                    })?;

                // Truncate to actual decoded sample count
                output.truncate(decoded_samples * channel_count);

                // Convert f32 slice to interleaved byte data
                let mut data = Vec::with_capacity(
                    decoded_samples * channel_count * std::mem::size_of::<f32>(),
                );
                for &sample in &output {
                    data.extend_from_slice(&sample.to_le_bytes());
                }

                self.pending_frame = Some(AudioFrame {
                    sample_rate: self.config.sample_rate,
                    channel_layout: self.config.channel_layout,
                    sample_format: SampleFormat::F32,
                    sample_count: decoded_samples as u32,
                    pts: pkt.pts,
                    data: vec![Bytes::from(data)],
                });
            }
            None => {
                // Opus doesn't buffer frames internally — mark flushing
                self.flushing = true;
                self.pending_frame = None;
            }
        }

        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError> {
        Ok(self.pending_frame.take())
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
    use splica_core::media::TrackIndex;
    use splica_core::Timestamp;

    #[test]
    fn test_that_opus_decoder_creates_with_valid_config() {
        let decoder = OpusDecoder::new(48000, ChannelLayout::Stereo);

        assert!(decoder.is_ok());
    }

    #[test]
    fn test_that_opus_decoder_config_is_accessible() {
        let decoder = OpusDecoder::new(48000, ChannelLayout::Mono).unwrap();
        let config = decoder.decoder_config();

        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channel_layout, ChannelLayout::Mono);
    }

    #[test]
    fn test_that_opus_decoder_rejects_surround() {
        let result = OpusDecoder::new(48000, ChannelLayout::Surround5_1);

        assert!(result.is_err());
    }

    #[test]
    fn test_that_opus_decoder_supports_downcasting() {
        let decoder = OpusDecoder::new(48000, ChannelLayout::Stereo).unwrap();
        let any_ref: &dyn Any = decoder.as_any();

        assert!(any_ref.downcast_ref::<OpusDecoder>().is_some());
    }

    #[test]
    fn test_that_opus_decoder_flush_returns_none() {
        let mut decoder = OpusDecoder::new(48000, ChannelLayout::Stereo).unwrap();

        decoder.send_packet(None).unwrap();

        assert!(decoder.receive_frame().unwrap().is_none());
    }

    #[test]
    fn test_that_opus_decoder_decodes_encoded_packet() {
        // GIVEN — encode a frame, then decode it
        use crate::opus::OpusEncoderBuilder;
        use splica_core::AudioEncoder;

        let mut encoder = OpusEncoderBuilder::new()
            .sample_rate(48000)
            .channel_layout(ChannelLayout::Stereo)
            .build()
            .unwrap();

        // 960 samples at 48000 Hz (20ms frame)
        let sample_count = 960u32;
        let channels = 2u32;
        let total_samples = sample_count * channels;
        let mut pcm_data = Vec::with_capacity(total_samples as usize * 4);
        for i in 0..total_samples {
            let t = i as f32 / (sample_count * channels) as f32;
            let sample = (t * std::f32::consts::TAU * 440.0).sin() * 0.5;
            pcm_data.extend_from_slice(&sample.to_le_bytes());
        }

        let frame = AudioFrame {
            sample_rate: 48000,
            channel_layout: ChannelLayout::Stereo,
            sample_format: SampleFormat::F32,
            sample_count,
            pts: Timestamp::new(0, 48000).unwrap(),
            data: vec![Bytes::from(pcm_data)],
        };

        encoder.send_frame(Some(&frame)).unwrap();
        let encoded_packet = encoder.receive_packet().unwrap().unwrap();

        // WHEN — decode the encoded packet
        let mut decoder = OpusDecoder::new(48000, ChannelLayout::Stereo).unwrap();
        let decode_packet = Packet {
            track_index: TrackIndex(0),
            pts: Timestamp::new(0, 48000).unwrap(),
            dts: Timestamp::new(0, 48000).unwrap(),
            is_keyframe: true,
            data: encoded_packet.data,
        };
        decoder.send_packet(Some(&decode_packet)).unwrap();
        let decoded_frame = decoder.receive_frame().unwrap();

        // THEN — decoded frame has 960 samples per channel
        assert!(decoded_frame.is_some());
        let audio_frame = decoded_frame.unwrap();
        assert_eq!(audio_frame.sample_count, 960);
        assert_eq!(audio_frame.sample_rate, 48000);
        assert_eq!(audio_frame.channel_layout, ChannelLayout::Stereo);
        assert_eq!(audio_frame.sample_format, SampleFormat::F32);
    }
}
