//! AV1 encoder implementation using rav1e.
//!
//! rav1e is a pure-Rust AV1 encoder (Apache 2.0). No FFI or unsafe code
//! is required — this module uses only the safe Rust API.

use std::any::Any;

use bytes::Bytes;
use rav1e::prelude::*;
use splica_core::error::EncodeError;
use splica_core::media::{
    ColorPrimaries, ColorRange, ColorSpace, Frame, MatrixCoefficients, Packet, PixelFormat,
    QualityTarget, TrackIndex, TransferCharacteristics, VideoFrame,
};
use splica_core::Encoder;

use crate::error::CodecError;

/// AV1 encoder wrapping rav1e.
///
/// Accepts YUV420p `VideoFrame`s and produces AV1 OBU encoded packets.
/// rav1e uses lookahead, so multiple frames may be buffered before
/// any output packets are produced. Flush via `send_frame(None)`.
pub struct Av1Encoder {
    inner: Context<u8>,
    config: Av1EncoderConfig,
    /// Track index to assign to output packets.
    track_index: TrackIndex,
    /// Buffered encoded packet (populated after receive_packet call to rav1e).
    pending_packet: Option<Packet>,
    /// Whether end-of-stream has been signaled.
    flushing: bool,
    /// Frame counter for PTS tracking.
    frame_count: u64,
}

/// AV1 encoder configuration parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Av1EncoderConfig {
    /// Target bitrate in bits per second (0 = constant quality mode).
    pub bitrate_bps: u32,
    /// Quantizer value for constant quality mode (0-255, lower = better).
    pub quantizer: u8,
    /// Speed preset (0 = slowest/best, 10 = fastest).
    pub speed: u8,
    /// Width of the encoded output (set after first frame).
    pub width: Option<u32>,
    /// Height of the encoded output (set after first frame).
    pub height: Option<u32>,
}

/// Builder for creating an `Av1Encoder` with specific settings.
pub struct Av1EncoderBuilder {
    bitrate_bps: u32,
    quantizer: u8,
    speed: u8,
    track_index: TrackIndex,
    max_frame_rate: Option<f32>,
    color_space: Option<ColorSpace>,
    width: u32,
    height: u32,
}

impl Av1EncoderBuilder {
    /// Creates a new encoder builder with default settings.
    ///
    /// Default: constant quality mode (quantizer 128), speed 6.
    pub fn new() -> Self {
        Self {
            bitrate_bps: 0,
            quantizer: 128,
            speed: 6,
            track_index: TrackIndex(0),
            max_frame_rate: None,
            color_space: None,
            width: 0,
            height: 0,
        }
    }

    /// Sets the target bitrate in bits per second.
    /// When set to non-zero, enables rate control mode.
    pub fn bitrate(mut self, bps: u32) -> Self {
        self.bitrate_bps = bps;
        self
    }

    /// Sets the quantizer for constant quality mode (0-255).
    pub fn quantizer(mut self, q: u8) -> Self {
        self.quantizer = q;
        self
    }

    /// Sets encoder quality from a `QualityTarget`.
    ///
    /// - `Bitrate(bps)` → enables rate control mode with the given bitrate.
    /// - `Crf(crf)` → maps CRF (0–51) to rav1e's quantizer (0–255) and
    ///   enables constant quality mode (bitrate = 0).
    pub fn quality(mut self, target: QualityTarget) -> Self {
        match target {
            QualityTarget::Bitrate(bps) => {
                self.bitrate_bps = bps;
            }
            QualityTarget::Crf(crf) => {
                // Map CRF 0–51 → quantizer 0–255 (linear scale).
                let crf = crf.min(QualityTarget::MAX_CRF);
                self.quantizer = (crf as u16 * 255 / 51) as u8;
                self.bitrate_bps = 0; // constant quality mode
            }
        }
        self
    }

    /// Sets the speed preset (0 = slowest/best, 10 = fastest).
    pub fn speed(mut self, speed: u8) -> Self {
        self.speed = speed;
        self
    }

    /// Sets the track index for output packets.
    pub fn track_index(mut self, index: TrackIndex) -> Self {
        self.track_index = index;
        self
    }

    /// Sets the maximum frame rate hint for the encoder.
    pub fn max_frame_rate(mut self, fps: f32) -> Self {
        self.max_frame_rate = Some(fps);
        self
    }

    /// Sets the color space for signaling in the output sequence header.
    pub fn color_space(mut self, cs: ColorSpace) -> Self {
        self.color_space = Some(cs);
        self
    }

    /// Sets the encode dimensions. Required before build.
    pub fn dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Builds the AV1 encoder.
    pub fn build(self) -> Result<Av1Encoder, CodecError> {
        if self.width == 0 || self.height == 0 {
            return Err(CodecError::InvalidConfig {
                message: "AV1 encoder requires non-zero width and height".to_string(),
            });
        }

        let speed = SpeedSettings::from_preset(self.speed.min(10));

        let time_base = if let Some(fps) = self.max_frame_rate {
            Rational::new(1, fps.round() as u64)
        } else {
            Rational::new(1, 30)
        };

        let mut enc_cfg = rav1e::EncoderConfig {
            width: self.width as usize,
            height: self.height as usize,
            bit_depth: 8,
            chroma_sampling: ChromaSampling::Cs420,
            chroma_sample_position: ChromaSamplePosition::Unknown,
            time_base,
            speed_settings: speed,
            quantizer: self.quantizer as usize,
            bitrate: self.bitrate_bps as i32,
            ..Default::default()
        };

        if let Some(cs) = &self.color_space {
            enc_cfg.pixel_range = match cs.range {
                ColorRange::Full => PixelRange::Full,
                ColorRange::Limited => PixelRange::Limited,
            };
            enc_cfg.color_description = Some(to_color_description(cs));
        }

        let cfg = Config::new().with_encoder_config(enc_cfg).with_threads(0);

        let inner = cfg
            .new_context::<u8>()
            .map_err(|e| CodecError::EncoderError {
                message: format!("failed to create rav1e encoder: {e}"),
            })?;

        Ok(Av1Encoder {
            inner,
            config: Av1EncoderConfig {
                bitrate_bps: self.bitrate_bps,
                quantizer: self.quantizer,
                speed: self.speed,
                width: Some(self.width),
                height: Some(self.height),
            },
            track_index: self.track_index,
            pending_packet: None,
            flushing: false,
            frame_count: 0,
        })
    }
}

impl Default for Av1EncoderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Av1Encoder {
    /// Returns a builder for configuring the encoder.
    pub fn builder() -> Av1EncoderBuilder {
        Av1EncoderBuilder::new()
    }

    /// Returns the encoder configuration.
    pub fn encoder_config(&self) -> &Av1EncoderConfig {
        &self.config
    }

    /// Returns the AV1 sequence header for container muxing.
    ///
    /// This should be written to the container as codec-private data
    /// (e.g., av1C in MP4, CodecPrivate in WebM/MKV).
    pub fn sequence_header(&self) -> Vec<u8> {
        self.inner.container_sequence_header()
    }

    /// Tries to receive a packet from rav1e and convert it to a splica Packet.
    fn try_receive(&mut self, fallback_pts: splica_core::Timestamp) -> Result<(), CodecError> {
        match self.inner.receive_packet() {
            Ok(pkt) => {
                let is_keyframe = pkt.frame_type == FrameType::KEY;

                // Use the input frame number to reconstruct PTS
                let pts =
                    splica_core::Timestamp::new(pkt.input_frameno as i64, fallback_pts.timebase())
                        .unwrap_or(fallback_pts);

                let packet = Packet {
                    track_index: self.track_index,
                    pts,
                    dts: pts,
                    is_keyframe,
                    data: Bytes::from(pkt.data),
                };

                self.pending_packet = Some(packet);
            }
            Err(EncoderStatus::NeedMoreData) | Err(EncoderStatus::Encoded) => {
                self.pending_packet = None;
            }
            Err(EncoderStatus::LimitReached) => {
                self.pending_packet = None;
            }
            Err(e) => {
                return Err(CodecError::EncoderError {
                    message: format!("rav1e receive_packet error: {e}"),
                });
            }
        }
        Ok(())
    }
}

impl Encoder for Av1Encoder {
    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<(), EncodeError> {
        match frame {
            Some(Frame::Video(video_frame)) => {
                if video_frame.pixel_format != PixelFormat::Yuv420p {
                    return Err(EncodeError::InvalidFrame {
                        message: format!(
                            "AV1 encoder requires Yuv420p, got {:?}",
                            video_frame.pixel_format
                        ),
                    });
                }

                if video_frame.planes.len() != 3 {
                    return Err(EncodeError::InvalidFrame {
                        message: format!(
                            "AV1 encoder requires 3 planes, got {}",
                            video_frame.planes.len()
                        ),
                    });
                }

                // Create a rav1e Frame and copy pixel data
                let mut rav1e_frame = self.inner.new_frame();
                copy_video_frame_to_rav1e(video_frame, &mut rav1e_frame);

                // Send frame to rav1e — handle EnoughData by draining first
                match self.inner.send_frame(rav1e_frame) {
                    Ok(()) => {}
                    Err(EncoderStatus::EnoughData) => {
                        // Drain a packet then retry
                        self.try_receive(video_frame.pts)?;
                        let retry_frame = self.inner.new_frame();
                        // Re-copy since we consumed the first frame
                        let mut retry = retry_frame;
                        copy_video_frame_to_rav1e(video_frame, &mut retry);
                        self.inner
                            .send_frame(retry)
                            .map_err(|e| CodecError::EncoderError {
                                message: format!("rav1e send_frame retry error: {e}"),
                            })?;
                    }
                    Err(e) => {
                        return Err(CodecError::EncoderError {
                            message: format!("rav1e send_frame error: {e}"),
                        }
                        .into());
                    }
                }

                // Try to receive a packet
                self.try_receive(video_frame.pts)?;
                self.frame_count += 1;
            }
            Some(Frame::Audio(_)) => {
                return Err(EncodeError::InvalidFrame {
                    message: "AV1 encoder received audio frame".to_string(),
                });
            }
            None => {
                // Flush — signal end of stream
                self.flushing = true;
                self.inner.flush();

                // Try to drain remaining packets
                let pts = splica_core::Timestamp::new(0, 1).unwrap();
                self.try_receive(pts)?;
            }
        }

        Ok(())
    }

    fn receive_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        // If we already have a pending packet from send_frame, return it
        if self.pending_packet.is_some() {
            return Ok(self.pending_packet.take());
        }

        // If flushing, keep draining rav1e
        if self.flushing {
            let pts = splica_core::Timestamp::new(0, 1).unwrap();
            self.try_receive(pts)
                .map_err(|e| EncodeError::Other(Box::new(e)))?;
            return Ok(self.pending_packet.take());
        }

        Ok(None)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Copies pixel data from a splica VideoFrame into a rav1e Frame.
fn copy_video_frame_to_rav1e(src: &VideoFrame, dst: &mut rav1e::Frame<u8>) {
    for (plane_idx, plane_layout) in src.planes.iter().enumerate() {
        let dst_plane = &mut dst.planes[plane_idx];
        let src_data = &src.data[plane_layout.offset..];

        for row in 0..plane_layout.height as usize {
            let src_start = row * plane_layout.stride;
            let src_end = src_start + plane_layout.width as usize;
            let src_row = &src_data[src_start..src_end];

            let dst_start = row * dst_plane.cfg.stride;
            let dst_end = dst_start + plane_layout.width as usize;
            dst_plane.data[dst_start..dst_end].copy_from_slice(src_row);
        }
    }
}

/// Converts a splica `ColorSpace` to a rav1e `ColorDescription`.
fn to_color_description(cs: &ColorSpace) -> ColorDescription {
    let color_primaries = match cs.primaries {
        ColorPrimaries::Bt709 => rav1e::color::ColorPrimaries::BT709,
        ColorPrimaries::Bt2020 => rav1e::color::ColorPrimaries::BT2020,
        ColorPrimaries::Smpte432 => rav1e::color::ColorPrimaries::SMPTE432,
    };
    let transfer_characteristics = match cs.transfer {
        TransferCharacteristics::Bt709 => rav1e::color::TransferCharacteristics::BT709,
        TransferCharacteristics::Smpte2084 => rav1e::color::TransferCharacteristics::SMPTE2084,
        TransferCharacteristics::HybridLogGamma => rav1e::color::TransferCharacteristics::HLG,
    };
    let matrix_coefficients = match cs.matrix {
        MatrixCoefficients::Bt709 => rav1e::color::MatrixCoefficients::BT709,
        MatrixCoefficients::Bt2020NonConstant => rav1e::color::MatrixCoefficients::BT2020NCL,
        MatrixCoefficients::Bt2020Constant => rav1e::color::MatrixCoefficients::BT2020CL,
        MatrixCoefficients::Identity => rav1e::color::MatrixCoefficients::Identity,
    };

    ColorDescription {
        color_primaries,
        transfer_characteristics,
        matrix_coefficients,
    }
}
