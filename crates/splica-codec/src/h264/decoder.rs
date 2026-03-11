//! H.264 decoder implementation using OpenH264.
//!
//! All `unsafe` code (via OpenH264 FFI) is contained within the `openh264` crate.
//! This module uses only the safe Rust API provided by that crate.

use std::any::Any;

use bytes::Bytes;
use splica_core::error::DecodeError;
use splica_core::media::{ColorSpace, Frame, Packet, PixelFormat, PlaneLayout, VideoFrame};
use splica_core::Decoder;

use openh264::formats::YUVSource;

use crate::error::CodecError;

use super::avcc::{self, AvcDecoderConfig};
use super::sps;

/// H.264 codec-specific configuration parameters.
///
/// Exposes H.264-specific details like profile, level, and reference frame count
/// that are hidden behind the generic `Decoder` trait. Use this when you need
/// broadcast compliance checks (Elena) or fine-grained codec control (Marcus).
///
/// # Example
///
/// ```ignore
/// use splica_core::Decoder;
/// use splica_codec::H264Decoder;
///
/// let decoder = H264Decoder::new(avcc_data).unwrap();
/// let config = decoder.codec_config();
///
/// // Check if the stream meets broadcast requirements
/// assert_eq!(config.profile, H264Profile::High);
/// assert!(config.level >= 40, "need at least level 4.0 for 1080p");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H264DecoderConfig {
    /// H.264 profile (Baseline, Main, High, etc.).
    pub profile: H264Profile,
    /// H.264 level as an integer (e.g., 30 = level 3.0, 40 = level 4.0).
    pub level: u8,
    /// Maximum number of reference frames indicated by the SPS.
    pub max_ref_frames: u8,
    /// NAL unit length size in bytes (typically 4).
    pub nal_length_size: u8,
}

/// H.264 profile identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum H264Profile {
    /// Constrained Baseline Profile (66).
    Baseline,
    /// Main Profile (77).
    Main,
    /// Extended Profile (88).
    Extended,
    /// High Profile (100).
    High,
    /// High 10 Profile (110).
    High10,
    /// High 4:2:2 Profile (122).
    High422,
    /// High 4:4:4 Predictive Profile (244).
    High444,
    /// Unknown profile indicator.
    Other(u8),
}

impl From<u8> for H264Profile {
    fn from(idc: u8) -> Self {
        match idc {
            66 => Self::Baseline,
            77 => Self::Main,
            88 => Self::Extended,
            100 => Self::High,
            110 => Self::High10,
            122 => Self::High422,
            244 => Self::High444,
            other => Self::Other(other),
        }
    }
}

/// H.264 decoder wrapping OpenH264.
///
/// Expects packets containing H.264 data in MP4 sample format (length-prefixed
/// NAL units). The avcC configuration record must be provided at construction
/// so that SPS/PPS can be fed to the decoder before any slice data.
pub struct H264Decoder {
    inner: openh264::decoder::Decoder,
    avcc_config: AvcDecoderConfig,
    /// Whether SPS/PPS have been sent to the decoder.
    initialized: bool,
    /// Buffered decoded frame (send/receive pattern: one frame per packet).
    pending_frame: Option<VideoFrame>,
    /// Whether end-of-stream has been signaled.
    flushing: bool,
    /// Color space parsed from the SPS VUI parameters.
    color_space: Option<ColorSpace>,
}

impl H264Decoder {
    /// Creates a new H.264 decoder from raw avcC box data.
    ///
    /// The `avcc_data` is the body of the avcC box from the MP4 sample description.
    /// It contains the SPS/PPS parameter sets needed to initialize the decoder.
    pub fn new(avcc_data: &[u8]) -> Result<Self, CodecError> {
        let avcc_config = AvcDecoderConfig::parse(avcc_data)?;

        // Extract color space from the first SPS VUI parameters, if present.
        let color_space = avcc_config
            .sps
            .first()
            .and_then(|sps_nalu| sps::parse_sps_color_info(sps_nalu))
            .map(|info| info.color_space);

        // SAFETY: openh264::decoder::Decoder::new() is a safe API that internally
        // calls WelsCreateDecoder via FFI. The openh264 crate manages the raw
        // pointer lifecycle through its Drop implementation.
        let inner = openh264::decoder::Decoder::new().map_err(|e| CodecError::DecoderError {
            message: format!("failed to create OpenH264 decoder: {e}"),
        })?;

        Ok(Self {
            inner,
            avcc_config,
            initialized: false,
            pending_frame: None,
            flushing: false,
            color_space,
        })
    }

    /// Returns the codec-specific configuration for this H.264 stream.
    ///
    /// Includes profile, level, and reference frame count extracted from
    /// the avcC record. Useful for broadcast compliance checks and
    /// codec parameter inspection.
    pub fn codec_config(&self) -> H264DecoderConfig {
        // Extract max_ref_frames from the first SPS if available.
        // In a minimal SPS, max_num_ref_frames is at a variable bit position
        // after profile/level/constraint bytes. For now, we expose a default
        // and can refine with proper SPS parsing later.
        // Full Exp-Golomb parsing of the SPS would be needed
        // to extract max_num_ref_frames accurately.
        // For now, return 0 to indicate "not parsed".
        let max_ref_frames = 0;

        H264DecoderConfig {
            profile: H264Profile::from(self.avcc_config.profile_idc),
            level: self.avcc_config.level_idc,
            max_ref_frames,
            nal_length_size: self.avcc_config.nal_length_size,
        }
    }

    /// Sends the SPS/PPS parameter sets to the decoder.
    fn initialize(&mut self) -> Result<(), CodecError> {
        let annex_b = self.avcc_config.to_annex_b();
        if !annex_b.is_empty() {
            // Decode the parameter sets — the decoder won't produce a frame,
            // but it needs these to interpret subsequent slice data.
            let _ = self
                .inner
                .decode(&annex_b)
                .map_err(|e| CodecError::DecoderError {
                    message: format!("failed to decode SPS/PPS: {e}"),
                })?;
        }
        self.initialized = true;
        Ok(())
    }

    /// Converts an OpenH264 decoded YUV frame into a splica `VideoFrame`.
    fn yuv_to_video_frame(
        yuv: &openh264::decoder::DecodedYUV<'_>,
        pts: splica_core::Timestamp,
        color_space: Option<ColorSpace>,
    ) -> Result<VideoFrame, CodecError> {
        let (width, height) = yuv.dimensions();
        let (uv_width, uv_height) = yuv.dimensions_uv();
        let (y_stride, u_stride, v_stride) = yuv.strides();

        let w = width as u32;
        let h = height as u32;
        let uvw = uv_width as u32;
        let uvh = uv_height as u32;

        // Copy plane data into a single contiguous buffer.
        // OpenH264 may use larger strides than the actual width, so we
        // copy row-by-row using only the valid pixel data width, but
        // preserve the stride in our layout for correct indexing.
        let y_plane_size = y_stride * height;
        let u_plane_size = u_stride * uv_height;
        let v_plane_size = v_stride * uv_height;
        let total_size = y_plane_size + u_plane_size + v_plane_size;

        let mut buf = Vec::with_capacity(total_size);

        // Y plane — copy stride*height bytes from the decoded buffer
        let y_data = yuv.y();
        buf.extend_from_slice(&y_data[..y_plane_size]);

        // U plane
        let u_data = yuv.u();
        buf.extend_from_slice(&u_data[..u_plane_size]);

        // V plane
        let v_data = yuv.v();
        buf.extend_from_slice(&v_data[..v_plane_size]);

        let y_offset = 0;
        let u_offset = y_plane_size;
        let v_offset = y_plane_size + u_plane_size;

        let planes = vec![
            PlaneLayout {
                offset: y_offset,
                stride: y_stride,
                width: w,
                height: h,
            },
            PlaneLayout {
                offset: u_offset,
                stride: u_stride,
                width: uvw,
                height: uvh,
            },
            PlaneLayout {
                offset: v_offset,
                stride: v_stride,
                width: uvw,
                height: uvh,
            },
        ];

        VideoFrame::new(
            w,
            h,
            PixelFormat::Yuv420p,
            color_space,
            pts,
            Bytes::from(buf),
            planes,
        )
        .map_err(|e| CodecError::DecoderError {
            message: format!("failed to create VideoFrame: {e}"),
        })
    }
}

impl Decoder for H264Decoder {
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError> {
        // Ensure SPS/PPS are sent before any slice data
        if !self.initialized {
            self.initialize()?;
        }

        match packet {
            Some(pkt) => {
                // Convert MP4 length-prefixed NAL units to Annex B format
                let annex_b = avcc::mp4_to_annex_b(&pkt.data, self.avcc_config.nal_length_size)?;

                // Decode the packet
                match self.inner.decode(&annex_b) {
                    Ok(Some(yuv)) => {
                        let frame = Self::yuv_to_video_frame(&yuv, pkt.pts, self.color_space)?;
                        self.pending_frame = Some(frame);
                    }
                    Ok(None) => {
                        // No frame produced yet (buffering, B-frame reordering, etc.)
                        self.pending_frame = None;
                    }
                    Err(e) => {
                        // OpenH264 docs say: don't terminate on first errors,
                        // continue decoding. We'll return the error but the caller
                        // can choose to continue.
                        return Err(CodecError::DecoderError {
                            message: format!("decode error: {e}"),
                        }
                        .into());
                    }
                }
            }
            None => {
                // End of stream — flush buffered frames
                self.flushing = true;
                match self.inner.flush_remaining() {
                    Ok(frames) => {
                        // Take the first flushed frame if any
                        if let Some(yuv) = frames.first() {
                            // Use a zero timestamp for flushed frames — caller
                            // should use the last known PTS
                            let pts = splica_core::Timestamp::new(0, 1).unwrap();
                            let frame = Self::yuv_to_video_frame(yuv, pts, self.color_space)?;
                            self.pending_frame = Some(frame);
                        } else {
                            self.pending_frame = None;
                        }
                    }
                    Err(e) => {
                        return Err(CodecError::DecoderError {
                            message: format!("flush error: {e}"),
                        }
                        .into());
                    }
                }
            }
        }

        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Option<Frame>, DecodeError> {
        Ok(self.pending_frame.take().map(Frame::Video))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
