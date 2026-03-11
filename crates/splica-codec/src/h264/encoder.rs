//! H.264 encoder implementation using OpenH264.
//!
//! All `unsafe` code (via OpenH264 FFI) is contained within the `openh264` crate.
//! This module uses only the safe Rust API provided by that crate.

use std::any::Any;

use bytes::Bytes;
use splica_core::error::EncodeError;
use splica_core::media::{
    ColorPrimaries, ColorRange, ColorSpace, Frame, MatrixCoefficients, Packet, PixelFormat,
    TrackIndex, TransferCharacteristics, VideoFrame,
};
use splica_core::Encoder;

use openh264::encoder::{EncoderConfig, FrameType, VuiConfig};
use openh264::formats::YUVSource;

use crate::error::CodecError;

/// H.264 encoder wrapping OpenH264.
///
/// Accepts YUV420p `VideoFrame`s and produces Annex B encoded packets.
/// The encoder uses the send/receive pattern: send frames via `send_frame()`,
/// then retrieve encoded packets via `receive_packet()`.
pub struct H264Encoder {
    inner: openh264::encoder::Encoder,
    config: H264EncoderConfig,
    /// Track index to assign to output packets.
    track_index: TrackIndex,
    /// Buffered encoded packet (one frame in → one packet out).
    pending_packet: Option<Packet>,
    /// Frame counter for generating DTS when no timestamp is available.
    frame_count: u64,
    /// Whether end-of-stream has been signaled.
    flushing: bool,
}

/// H.264 encoder configuration parameters.
///
/// Exposes encoder-specific settings like bitrate, profile, and level.
/// Access via downcasting: `encoder.as_any().downcast_ref::<H264Encoder>()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H264EncoderConfig {
    /// Target bitrate in bits per second.
    pub bitrate_bps: u32,
    /// H.264 profile (if explicitly set).
    pub profile: Option<H264EncoderProfile>,
    /// H.264 level (if explicitly set).
    pub level: Option<H264EncoderLevel>,
    /// Width of the encoded output (set after first frame).
    pub width: Option<u32>,
    /// Height of the encoded output (set after first frame).
    pub height: Option<u32>,
}

/// H.264 encoding profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum H264EncoderProfile {
    Baseline,
    Main,
    High,
}

/// H.264 encoding level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum H264EncoderLevel {
    Level3_0,
    Level3_1,
    Level4_0,
    Level4_1,
    Level5_0,
    Level5_1,
}

/// Builder for creating an `H264Encoder` with specific settings.
pub struct H264EncoderBuilder {
    bitrate_bps: u32,
    profile: Option<H264EncoderProfile>,
    level: Option<H264EncoderLevel>,
    track_index: TrackIndex,
    max_frame_rate: Option<f32>,
    color_space: Option<ColorSpace>,
}

impl H264EncoderBuilder {
    /// Creates a new encoder builder with default settings.
    ///
    /// Default bitrate is 1 Mbps.
    pub fn new() -> Self {
        Self {
            bitrate_bps: 1_000_000,
            profile: None,
            level: None,
            track_index: TrackIndex(0),
            max_frame_rate: None,
            color_space: None,
        }
    }

    /// Sets the target bitrate in bits per second.
    pub fn bitrate(mut self, bps: u32) -> Self {
        self.bitrate_bps = bps;
        self
    }

    /// Sets the H.264 encoding profile.
    pub fn profile(mut self, profile: H264EncoderProfile) -> Self {
        self.profile = Some(profile);
        self
    }

    /// Sets the H.264 encoding level.
    pub fn level(mut self, level: H264EncoderLevel) -> Self {
        self.level = Some(level);
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

    /// Sets the color space for VUI signaling in the output SPS.
    pub fn color_space(mut self, cs: ColorSpace) -> Self {
        self.color_space = Some(cs);
        self
    }

    /// Builds the H.264 encoder.
    pub fn build(self) -> Result<H264Encoder, CodecError> {
        let mut enc_config =
            EncoderConfig::new().bitrate(openh264::encoder::BitRate::from_bps(self.bitrate_bps));

        if let Some(fps) = self.max_frame_rate {
            enc_config = enc_config.max_frame_rate(openh264::encoder::FrameRate::from_hz(fps));
        }

        if let Some(profile) = self.profile {
            let oh264_profile = match profile {
                H264EncoderProfile::Baseline => openh264::encoder::Profile::Baseline,
                H264EncoderProfile::Main => openh264::encoder::Profile::Main,
                H264EncoderProfile::High => openh264::encoder::Profile::High,
            };
            enc_config = enc_config.profile(oh264_profile);
        }

        if let Some(level) = self.level {
            let oh264_level = match level {
                H264EncoderLevel::Level3_0 => openh264::encoder::Level::Level_3_0,
                H264EncoderLevel::Level3_1 => openh264::encoder::Level::Level_3_1,
                H264EncoderLevel::Level4_0 => openh264::encoder::Level::Level_4_0,
                H264EncoderLevel::Level4_1 => openh264::encoder::Level::Level_4_1,
                H264EncoderLevel::Level5_0 => openh264::encoder::Level::Level_5_0,
                H264EncoderLevel::Level5_1 => openh264::encoder::Level::Level_5_1,
            };
            enc_config = enc_config.level(oh264_level);
        }

        if let Some(cs) = &self.color_space {
            enc_config = enc_config.vui(to_vui_config(cs));
        }

        let api = openh264::OpenH264API::from_source();
        let inner = openh264::encoder::Encoder::with_api_config(api, enc_config).map_err(|e| {
            CodecError::EncoderError {
                message: format!("failed to create OpenH264 encoder: {e}"),
            }
        })?;

        Ok(H264Encoder {
            inner,
            config: H264EncoderConfig {
                bitrate_bps: self.bitrate_bps,
                profile: self.profile,
                level: self.level,
                width: None,
                height: None,
            },
            track_index: self.track_index,
            pending_packet: None,
            frame_count: 0,
            flushing: false,
        })
    }
}

impl Default for H264EncoderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl H264Encoder {
    /// Creates a new H.264 encoder with default settings (1 Mbps, auto profile/level).
    pub fn new() -> Result<Self, CodecError> {
        H264EncoderBuilder::new().build()
    }

    /// Returns a builder for configuring the encoder.
    pub fn builder() -> H264EncoderBuilder {
        H264EncoderBuilder::new()
    }

    /// Returns the encoder configuration.
    pub fn encoder_config(&self) -> &H264EncoderConfig {
        &self.config
    }
}

impl Encoder for H264Encoder {
    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<(), EncodeError> {
        match frame {
            Some(Frame::Video(video_frame)) => {
                if video_frame.pixel_format != PixelFormat::Yuv420p {
                    return Err(EncodeError::InvalidFrame {
                        message: format!(
                            "H.264 encoder requires Yuv420p, got {:?}",
                            video_frame.pixel_format
                        ),
                    });
                }

                if video_frame.planes.len() != 3 {
                    return Err(EncodeError::InvalidFrame {
                        message: format!(
                            "H.264 encoder requires 3 planes, got {}",
                            video_frame.planes.len()
                        ),
                    });
                }

                // Update config with actual dimensions
                self.config.width = Some(video_frame.width);
                self.config.height = Some(video_frame.height);

                // Create a YUVSource adapter for the VideoFrame
                let yuv_adapter = VideoFrameYuv { frame: video_frame };

                let encoded =
                    self.inner
                        .encode(&yuv_adapter)
                        .map_err(|e| CodecError::EncoderError {
                            message: format!("encode error: {e}"),
                        })?;

                let frame_type = encoded.frame_type();

                // Skip frames that the encoder decided to drop
                if frame_type == FrameType::Skip || frame_type == FrameType::Invalid {
                    self.pending_packet = None;
                    self.frame_count += 1;
                    return Ok(());
                }

                // Collect all NAL units into a single Annex B bitstream
                let annex_b_data = encoded.to_vec();

                let is_keyframe = frame_type == FrameType::IDR || frame_type == FrameType::I;

                let packet = Packet {
                    track_index: self.track_index,
                    pts: video_frame.pts,
                    dts: video_frame.pts, // OpenH264 Baseline doesn't reorder
                    is_keyframe,
                    data: Bytes::from(annex_b_data),
                };

                self.pending_packet = Some(packet);
                self.frame_count += 1;
            }
            Some(Frame::Audio(_)) => {
                return Err(EncodeError::InvalidFrame {
                    message: "H.264 encoder received audio frame".to_string(),
                });
            }
            None => {
                // End of stream — OpenH264 doesn't buffer frames (no B-frame reordering
                // in Baseline profile), so there's nothing to flush.
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

/// Converts a splica `ColorSpace` into an OpenH264 `VuiConfig`.
fn to_vui_config(cs: &ColorSpace) -> VuiConfig {
    let primaries = match cs.primaries {
        ColorPrimaries::Bt709 => openh264::encoder::ColorPrimaries::Bt709,
        ColorPrimaries::Bt2020 => openh264::encoder::ColorPrimaries::Bt2020,
        ColorPrimaries::Smpte432 => openh264::encoder::ColorPrimaries::Bt709,
    };
    let transfer = match cs.transfer {
        TransferCharacteristics::Bt709 => openh264::encoder::TransferCharacteristics::Bt709,
        TransferCharacteristics::Smpte2084 => openh264::encoder::TransferCharacteristics::Smpte2084,
        TransferCharacteristics::HybridLogGamma => openh264::encoder::TransferCharacteristics::Hlg,
    };
    let matrix = match cs.matrix {
        MatrixCoefficients::Bt709 => openh264::encoder::MatrixCoefficients::Bt709,
        MatrixCoefficients::Bt2020NonConstant => openh264::encoder::MatrixCoefficients::Bt2020Ncl,
        MatrixCoefficients::Bt2020Constant => openh264::encoder::MatrixCoefficients::Bt2020Cl,
        MatrixCoefficients::Identity => openh264::encoder::MatrixCoefficients::Identity,
    };
    let full_range = matches!(cs.range, ColorRange::Full);

    VuiConfig::new()
        .color_primaries(primaries)
        .transfer_characteristics(transfer)
        .matrix_coefficients(matrix)
        .full_range(full_range)
}

/// Adapter that implements `openh264::formats::YUVSource` for a `VideoFrame`.
///
/// This bridges the splica `VideoFrame` (with `Bytes` data and `PlaneLayout`)
/// to OpenH264's expected YUV input format.
struct VideoFrameYuv<'a> {
    frame: &'a VideoFrame,
}

impl YUVSource for VideoFrameYuv<'_> {
    fn dimensions(&self) -> (usize, usize) {
        (self.frame.width as usize, self.frame.height as usize)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (
            self.frame.planes[0].stride,
            self.frame.planes[1].stride,
            self.frame.planes[2].stride,
        )
    }

    fn y(&self) -> &[u8] {
        let plane = &self.frame.planes[0];
        let end = plane.offset + plane.stride * plane.height as usize;
        &self.frame.data[plane.offset..end]
    }

    fn u(&self) -> &[u8] {
        let plane = &self.frame.planes[1];
        let end = plane.offset + plane.stride * plane.height as usize;
        &self.frame.data[plane.offset..end]
    }

    fn v(&self) -> &[u8] {
        let plane = &self.frame.planes[2];
        let end = plane.offset + plane.stride * plane.height as usize;
        &self.frame.data[plane.offset..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use splica_core::media::{ColorSpace, PlaneLayout};
    use splica_core::Timestamp;

    /// Create a synthetic YUV420p VideoFrame with a solid color.
    fn make_test_frame(width: u32, height: u32, pts_ticks: i64) -> VideoFrame {
        let y_stride = width as usize;
        let uv_stride = (width / 2) as usize;
        let y_size = y_stride * height as usize;
        let uv_size = uv_stride * (height / 2) as usize;

        let mut data = vec![0u8; y_size + uv_size * 2];
        // Fill Y plane with mid-gray
        for b in &mut data[..y_size] {
            *b = 128;
        }
        // Fill U and V planes with neutral chroma
        for b in &mut data[y_size..y_size + uv_size] {
            *b = 128;
        }
        for b in &mut data[y_size + uv_size..] {
            *b = 128;
        }

        VideoFrame::new(
            width,
            height,
            PixelFormat::Yuv420p,
            ColorSpace::BT709,
            Timestamp::new(pts_ticks, 30000),
            Bytes::from(data),
            vec![
                PlaneLayout {
                    offset: 0,
                    stride: y_stride,
                    width,
                    height,
                },
                PlaneLayout {
                    offset: y_size,
                    stride: uv_stride,
                    width: width / 2,
                    height: height / 2,
                },
                PlaneLayout {
                    offset: y_size + uv_size,
                    stride: uv_stride,
                    width: width / 2,
                    height: height / 2,
                },
            ],
        )
        .unwrap()
    }

    #[test]
    fn test_that_encoder_produces_packets_from_frames() {
        // GIVEN — an encoder and a synthetic video frame
        let mut encoder = H264Encoder::new().unwrap();
        let frame = make_test_frame(128, 128, 0);

        // WHEN — send a frame and receive a packet
        encoder.send_frame(Some(&Frame::Video(frame))).unwrap();
        let packet = encoder.receive_packet().unwrap();

        // THEN — a non-empty packet is produced
        assert!(packet.is_some());
        let pkt = packet.unwrap();
        assert!(!pkt.data.is_empty());
        assert!(pkt.is_keyframe); // first frame should be IDR
    }

    #[test]
    fn test_that_encoder_produces_multiple_packets() {
        // GIVEN — an encoder and multiple frames
        let mut encoder = H264Encoder::new().unwrap();

        // WHEN — encode 5 frames
        let mut packets = Vec::new();
        for i in 0..5 {
            let frame = make_test_frame(128, 128, i * 1001);
            encoder.send_frame(Some(&Frame::Video(frame))).unwrap();
            if let Some(pkt) = encoder.receive_packet().unwrap() {
                packets.push(pkt);
            }
        }

        // THEN — at least some packets produced (encoder may skip some)
        assert!(!packets.is_empty());
        // First packet should be a keyframe
        assert!(packets[0].is_keyframe);
    }

    #[test]
    fn test_that_encoder_flush_produces_no_extra_packets() {
        // GIVEN — an encoder that has encoded one frame
        let mut encoder = H264Encoder::new().unwrap();
        let frame = make_test_frame(128, 128, 0);
        encoder.send_frame(Some(&Frame::Video(frame))).unwrap();
        let _ = encoder.receive_packet().unwrap();

        // WHEN — flush the encoder
        encoder.send_frame(None).unwrap();
        let flushed = encoder.receive_packet().unwrap();

        // THEN — OpenH264 Baseline has no B-frame reordering, so nothing to flush
        assert!(flushed.is_none());
    }

    #[test]
    fn test_that_encoder_rejects_non_yuv420p() {
        // GIVEN — an encoder and a frame with wrong pixel format (YUV422p)
        let mut encoder = H264Encoder::new().unwrap();
        let y_size = 128 * 128;
        let uv_size = 64 * 128; // 4:2:2 has full-height chroma
        let frame = VideoFrame::new(
            128,
            128,
            PixelFormat::Yuv422p,
            ColorSpace::BT709,
            Timestamp::new(0, 30000),
            Bytes::from(vec![0u8; y_size + uv_size * 2]),
            vec![
                PlaneLayout {
                    offset: 0,
                    stride: 128,
                    width: 128,
                    height: 128,
                },
                PlaneLayout {
                    offset: y_size,
                    stride: 64,
                    width: 64,
                    height: 128,
                },
                PlaneLayout {
                    offset: y_size + uv_size,
                    stride: 64,
                    width: 64,
                    height: 128,
                },
            ],
        )
        .unwrap();

        // WHEN
        let result = encoder.send_frame(Some(&Frame::Video(frame)));

        // THEN
        assert!(result.is_err());
    }

    #[test]
    fn test_that_encoder_config_is_accessible_via_downcast() {
        // GIVEN — an encoder with custom bitrate
        let encoder = H264EncoderBuilder::new()
            .bitrate(2_000_000)
            .build()
            .unwrap();

        // WHEN — access config via as_any downcast
        let any_ref: &dyn Any = encoder.as_any();
        let h264 = any_ref.downcast_ref::<H264Encoder>().unwrap();

        // THEN
        assert_eq!(h264.encoder_config().bitrate_bps, 2_000_000);
    }
}
