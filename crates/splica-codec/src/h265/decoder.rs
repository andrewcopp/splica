//! H.265 decoder implementation using libde265.
//!
//! All `unsafe` code is contained within the `libde265` crate.
//! This module uses only the safe Rust API provided by that crate.

use bytes::Bytes;
use splica_core::error::DecodeError;
use splica_core::media::{
    ColorRange, ColorSpace, Frame, Packet, PixelFormat, PlaneLayout, VideoFrame,
};
use splica_core::Decoder;
use std::any::Any;

use crate::color::{map_color_primaries, map_matrix_coefficients, map_transfer_characteristics};
use crate::error::CodecError;
use crate::h264::avcc::mp4_to_annex_b;

use super::hvcc::HevcDecoderConfig;

/// H.265 codec-specific configuration parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H265DecoderConfig {
    /// HEVC general profile IDC (e.g., 1 = Main, 2 = Main 10).
    pub general_profile_idc: u8,
    /// HEVC general level IDC (e.g., 93 = level 3.1).
    pub general_level_idc: u8,
    /// NAL unit length size in bytes (typically 4).
    pub nal_length_size: u8,
}

/// H.265 decoder wrapping libde265.
///
/// Expects packets containing H.265 data in MP4 sample format (length-prefixed
/// NAL units). The hvcC configuration record must be provided at construction
/// so that VPS/SPS/PPS can be fed to the decoder before any slice data.
pub struct H265Decoder {
    inner: libde265::Decoder,
    hvcc_config: HevcDecoderConfig,
    /// Whether VPS/SPS/PPS have been sent to the decoder.
    initialized: bool,
    /// Buffered decoded frame (send/receive pattern: one frame per packet).
    pending_frame: Option<VideoFrame>,
    /// Whether end-of-stream has been signaled.
    flushing: bool,
}

impl H265Decoder {
    /// Creates a new H.265 decoder from raw hvcC box data.
    ///
    /// The `hvcc_data` is the body of the hvcC box from the MP4 sample description.
    /// It contains the VPS/SPS/PPS parameter sets needed to initialize the decoder.
    pub fn new(hvcc_data: &[u8]) -> Result<Self, CodecError> {
        let hvcc_config = HevcDecoderConfig::parse(hvcc_data)?;

        let session = libde265::De265::new().map_err(|e| CodecError::DecoderError {
            message: format!("failed to create libde265 session: {e}"),
        })?;

        let inner = libde265::Decoder::new(session);

        Ok(Self {
            inner,
            hvcc_config,
            initialized: false,
            pending_frame: None,
            flushing: false,
        })
    }

    /// Returns the codec-specific configuration for this H.265 stream.
    pub fn codec_config(&self) -> H265DecoderConfig {
        H265DecoderConfig {
            general_profile_idc: self.hvcc_config.general_profile_idc,
            general_level_idc: self.hvcc_config.general_level_idc,
            nal_length_size: self.hvcc_config.nal_length_size,
        }
    }

    /// Sends the VPS/SPS/PPS parameter sets to the decoder.
    fn initialize(&mut self) -> Result<(), CodecError> {
        let annex_b = self.hvcc_config.to_annex_b();
        if !annex_b.is_empty() {
            self.inner
                .push_data(&annex_b, 0, None)
                .map_err(|e| CodecError::DecoderError {
                    message: format!("failed to push VPS/SPS/PPS: {e}"),
                })?;
            // Decode the parameter sets — no frame will be produced
            let _ = self.inner.decode();
        }
        self.initialized = true;
        Ok(())
    }

    /// Extracts color space from a decoded libde265 Image using its VUI fields.
    fn extract_color_space(image: &libde265::Image) -> Option<ColorSpace> {
        let primaries = map_color_primaries(image.get_image_colour_primaries() as u8)?;
        let transfer =
            map_transfer_characteristics(image.get_image_transfer_characteristics() as u8)?;
        let matrix = map_matrix_coefficients(image.get_image_matrix_coefficients() as u8)?;
        let range = if image.get_image_full_range_flag() == 1 {
            ColorRange::Full
        } else {
            ColorRange::Limited
        };

        Some(ColorSpace {
            primaries,
            transfer,
            matrix,
            range,
        })
    }

    /// Converts a libde265 Image into a splica `VideoFrame`.
    fn image_to_video_frame(
        image: &libde265::Image,
        pts: splica_core::Timestamp,
    ) -> Result<VideoFrame, CodecError> {
        let w = image.get_image_width(0);
        let h = image.get_image_height(0);
        let uvw = image.get_image_width(1);
        let uvh = image.get_image_height(1);

        let (y_data, y_stride) = image.get_image_plane(0);
        let (u_data, u_stride) = image.get_image_plane(1);
        let (v_data, v_stride) = image.get_image_plane(2);

        let y_plane_size = y_stride * h as usize;
        let u_plane_size = u_stride * uvh as usize;
        let v_plane_size = v_stride * uvh as usize;
        let total_size = y_plane_size + u_plane_size + v_plane_size;

        let mut buf = Vec::with_capacity(total_size);
        buf.extend_from_slice(&y_data[..y_plane_size]);
        buf.extend_from_slice(&u_data[..u_plane_size]);
        buf.extend_from_slice(&v_data[..v_plane_size]);

        let y_offset = 0;
        let u_offset = y_plane_size;
        let v_offset = y_plane_size + u_plane_size;

        let color_space = Self::extract_color_space(image);

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

impl Decoder for H265Decoder {
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError> {
        if !self.initialized {
            self.initialize()?;
        }

        match packet {
            Some(pkt) => {
                // Convert MP4 length-prefixed NAL units to Annex B format
                let annex_b = mp4_to_annex_b(&pkt.data, self.hvcc_config.nal_length_size)?;

                let pts_us = (pkt.pts.as_seconds_f64() * 1_000_000.0) as i64;
                self.inner.push_data(&annex_b, pts_us, None).map_err(|e| {
                    CodecError::DecoderError {
                        message: format!("push_data error: {e}"),
                    }
                })?;

                // Decode available data
                match self.inner.decode() {
                    Ok(()) => {}
                    Err(libde265::Error::WaitingForInputData) => {}
                    Err(e) => {
                        return Err(CodecError::DecoderError {
                            message: format!("decode error: {e}"),
                        }
                        .into());
                    }
                }

                // Try to get a decoded picture
                if let Some(image) = self.inner.get_next_picture() {
                    let frame = Self::image_to_video_frame(&image, pkt.pts)?;
                    self.pending_frame = Some(frame);
                } else {
                    self.pending_frame = None;
                }
            }
            None => {
                // End of stream — flush
                self.flushing = true;
                let _ = self.inner.flush_data();
                match self.inner.decode() {
                    Ok(()) => {}
                    Err(libde265::Error::WaitingForInputData) => {}
                    Err(e) => {
                        return Err(CodecError::DecoderError {
                            message: format!("flush decode error: {e}"),
                        }
                        .into());
                    }
                }

                if let Some(image) = self.inner.get_next_picture() {
                    // Recover PTS from libde265 (stored as microseconds via push_data)
                    let pts_us = image.get_image_pts();
                    let pts = splica_core::Timestamp::new(pts_us, 1_000_000)
                        .unwrap_or_else(|| splica_core::Timestamp::new(0, 1).unwrap());
                    let frame = Self::image_to_video_frame(&image, pts)?;
                    self.pending_frame = Some(frame);
                } else {
                    self.pending_frame = None;
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
