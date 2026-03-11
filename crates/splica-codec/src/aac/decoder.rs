//! AAC decoder implementation using symphonia (pure Rust).
//!
//! Decodes AAC-LC audio packets into raw PCM frames. Uses symphonia's
//! pure-Rust AAC decoder, making this WASM-compatible without any FFI.

use std::any::Any;

use bytes::Bytes;
use splica_core::error::DecodeError;
use splica_core::media::{AudioFrame, ChannelLayout, Packet, SampleFormat};
use splica_core::AudioDecoder;
use symphonia_core::audio::{AudioBufferRef, Signal};
use symphonia_core::codecs::{CodecParameters, Decoder, DecoderOptions, CODEC_TYPE_AAC};

use crate::error::CodecError;

/// AAC codec-specific configuration extracted from AudioSpecificConfig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AacDecoderConfig {
    /// Sample rate in Hz (e.g., 44100, 48000).
    pub sample_rate: u32,
    /// Number of audio channels.
    pub channels: u32,
}

/// AAC decoder wrapping symphonia's pure-Rust AAC-LC decoder.
///
/// Expects packets containing raw AAC frames (as extracted from an MP4/WebM
/// container). The AudioSpecificConfig must be provided at construction
/// so that the decoder can determine sample rate and channel configuration.
pub struct AacDecoder {
    inner: symphonia_codec_aac::AacDecoder,
    sample_rate: u32,
    channel_layout: ChannelLayout,
    pending_frame: Option<AudioFrame>,
    flushing: bool,
}

impl AacDecoder {
    /// Creates a new AAC decoder from raw AudioSpecificConfig bytes.
    ///
    /// The `asc_bytes` are typically extracted from the `esds` box in an MP4
    /// container (the DecoderSpecificInfo field).
    pub fn new(asc_bytes: &[u8]) -> Result<Self, CodecError> {
        let mut params = CodecParameters::new();
        params
            .for_codec(CODEC_TYPE_AAC)
            .with_extra_data(asc_bytes.to_vec().into_boxed_slice());

        let options = DecoderOptions::default();
        let inner = symphonia_codec_aac::AacDecoder::try_new(&params, &options).map_err(|e| {
            CodecError::InvalidConfig {
                message: format!("failed to create AAC decoder: {e}"),
            }
        })?;

        // Parse basic info from AudioSpecificConfig (2-byte minimum).
        // Byte 0: [objectType:5][freqIndex:4(high3)]
        // Byte 1: [freqIndex:4(low1)][channelConfig:4]
        let (sample_rate, channel_layout) = if asc_bytes.len() >= 2 {
            let freq_index = ((asc_bytes[0] & 0x07) << 1) | (asc_bytes[1] >> 7);
            let channel_config = (asc_bytes[1] >> 3) & 0x0F;

            let sr = match freq_index {
                0 => 96000,
                1 => 88200,
                2 => 64000,
                3 => 48000,
                4 => 44100,
                5 => 32000,
                6 => 24000,
                7 => 22050,
                8 => 16000,
                9 => 12000,
                10 => 11025,
                11 => 8000,
                12 => 7350,
                _ => 44100, // fallback
            };

            let layout = match channel_config {
                1 => ChannelLayout::Mono,
                2 => ChannelLayout::Stereo,
                6 => ChannelLayout::Surround5_1,
                7 => ChannelLayout::Surround7_1,
                _ => ChannelLayout::Stereo, // fallback
            };

            (sr, layout)
        } else {
            (44100, ChannelLayout::Stereo)
        };

        Ok(Self {
            inner,
            sample_rate,
            channel_layout,
            pending_frame: None,
            flushing: false,
        })
    }

    /// Returns the codec-specific configuration for this AAC stream.
    pub fn codec_config(&self) -> AacDecoderConfig {
        AacDecoderConfig {
            sample_rate: self.sample_rate,
            channels: self.channel_layout.channel_count(),
        }
    }

    /// Converts a symphonia audio buffer into a splica `AudioFrame`.
    fn buffer_to_audio_frame(
        buf_ref: AudioBufferRef<'_>,
        pts: splica_core::Timestamp,
        sample_rate: u32,
        channel_layout: ChannelLayout,
    ) -> Result<AudioFrame, CodecError> {
        let num_channels = channel_layout.channel_count() as usize;
        let num_frames = buf_ref.frames();

        // Symphonia outputs planar F32 for AAC. Convert to interleaved F32
        // which is splica's F32 SampleFormat.
        match buf_ref {
            AudioBufferRef::F32(buf) => {
                let spec = buf.spec();
                let actual_channels = spec.channels.count();
                let channels_to_use = num_channels.min(actual_channels);

                // Interleave: [L0, R0, L1, R1, ...]
                let mut interleaved =
                    Vec::with_capacity(num_frames * channels_to_use * std::mem::size_of::<f32>());
                for frame_idx in 0..num_frames {
                    for ch in 0..channels_to_use {
                        let sample = buf.chan(ch)[frame_idx];
                        interleaved.extend_from_slice(&sample.to_le_bytes());
                    }
                }

                Ok(AudioFrame {
                    sample_rate,
                    channel_layout,
                    sample_format: SampleFormat::F32,
                    sample_count: num_frames as u32,
                    pts,
                    data: vec![Bytes::from(interleaved)],
                })
            }
            AudioBufferRef::F64(buf) => {
                let spec = buf.spec();
                let actual_channels = spec.channels.count();
                let channels_to_use = num_channels.min(actual_channels);

                // Downconvert F64 to F32 interleaved
                let mut interleaved =
                    Vec::with_capacity(num_frames * channels_to_use * std::mem::size_of::<f32>());
                for frame_idx in 0..num_frames {
                    for ch in 0..channels_to_use {
                        let sample = buf.chan(ch)[frame_idx] as f32;
                        interleaved.extend_from_slice(&sample.to_le_bytes());
                    }
                }

                Ok(AudioFrame {
                    sample_rate,
                    channel_layout,
                    sample_format: SampleFormat::F32,
                    sample_count: num_frames as u32,
                    pts,
                    data: vec![Bytes::from(interleaved)],
                })
            }
            AudioBufferRef::S32(buf) => {
                let spec = buf.spec();
                let actual_channels = spec.channels.count();
                let channels_to_use = num_channels.min(actual_channels);

                // Convert S32 to F32 interleaved
                let mut interleaved =
                    Vec::with_capacity(num_frames * channels_to_use * std::mem::size_of::<f32>());
                for frame_idx in 0..num_frames {
                    for ch in 0..channels_to_use {
                        let sample = buf.chan(ch)[frame_idx] as f32 / i32::MAX as f32;
                        interleaved.extend_from_slice(&sample.to_le_bytes());
                    }
                }

                Ok(AudioFrame {
                    sample_rate,
                    channel_layout,
                    sample_format: SampleFormat::F32,
                    sample_count: num_frames as u32,
                    pts,
                    data: vec![Bytes::from(interleaved)],
                })
            }
            _ => Err(CodecError::Unsupported {
                message: "unsupported symphonia audio buffer format".to_string(),
            }),
        }
    }
}

impl AudioDecoder for AacDecoder {
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError> {
        match packet {
            Some(pkt) => {
                let sym_packet = symphonia_core::formats::Packet::new_from_boxed_slice(
                    0, // track id
                    pkt.pts.ticks() as u64,
                    0, // duration (unknown)
                    pkt.data.to_vec().into_boxed_slice(),
                );

                match self.inner.decode(&sym_packet) {
                    Ok(buf_ref) => {
                        let frame = Self::buffer_to_audio_frame(
                            buf_ref,
                            pkt.pts,
                            self.sample_rate,
                            self.channel_layout,
                        )?;
                        self.pending_frame = Some(frame);
                    }
                    Err(symphonia_core::errors::Error::DecodeError(msg)) => {
                        return Err(CodecError::DecoderError {
                            message: format!("AAC decode error: {msg}"),
                        }
                        .into());
                    }
                    Err(e) => {
                        return Err(CodecError::DecoderError {
                            message: format!("AAC decode error: {e}"),
                        }
                        .into());
                    }
                }
            }
            None => {
                // End of stream — AAC doesn't buffer frames, so just mark flushing
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

    // AAC-LC, 44100 Hz, Stereo
    // objectType=2 (AAC-LC), freqIndex=4 (44100), channelConfig=2 (stereo)
    // Binary: 00010_0100_010_00000 = 0x12 0x10
    const AAC_LC_44100_STEREO: &[u8] = &[0x12, 0x10];

    // AAC-LC, 48000 Hz, Mono
    // objectType=2, freqIndex=3 (48000), channelConfig=1 (mono)
    // Binary: 00010_0011_001_00000 = 0x11, 0x88
    const AAC_LC_48000_MONO: &[u8] = &[0x11, 0x88];

    #[test]
    fn test_that_aac_decoder_creates_from_valid_asc() {
        let decoder = AacDecoder::new(AAC_LC_44100_STEREO);

        assert!(decoder.is_ok());
    }

    #[test]
    fn test_that_aac_decoder_parses_44100_stereo_config() {
        let decoder = AacDecoder::new(AAC_LC_44100_STEREO).unwrap();
        let config = decoder.codec_config();

        assert_eq!(config.sample_rate, 44100);
        assert_eq!(config.channels, 2);
    }

    #[test]
    fn test_that_aac_decoder_parses_48000_mono_config() {
        let decoder = AacDecoder::new(AAC_LC_48000_MONO).unwrap();
        let config = decoder.codec_config();

        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channels, 1);
    }

    #[test]
    fn test_that_aac_decoder_supports_downcasting() {
        let decoder = AacDecoder::new(AAC_LC_44100_STEREO).unwrap();
        let any_ref: &dyn Any = decoder.as_any();

        assert!(any_ref.downcast_ref::<AacDecoder>().is_some());
    }

    #[test]
    fn test_that_aac_decoder_flush_returns_none() {
        let mut decoder = AacDecoder::new(AAC_LC_44100_STEREO).unwrap();

        // Signal end-of-stream
        decoder.send_packet(None).unwrap();

        // No buffered frames to flush
        assert!(decoder.receive_frame().unwrap().is_none());
    }
}
