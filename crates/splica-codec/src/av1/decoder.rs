//! AV1 decoder implementation using dav1d.
//!
//! All `unsafe` code is contained within the `dav1d` crate.
//! This module uses only the safe Rust API provided by that crate.

use bytes::Bytes;
use dav1d::PlanarImageComponent;
use num_traits::ToPrimitive;
use splica_core::error::DecodeError;
use splica_core::media::{
    ColorRange, ColorSpace, Frame, Packet, PixelFormat, PlaneLayout, VideoFrame,
};
use splica_core::Decoder;
use std::any::Any;

use crate::color::{map_color_primaries, map_matrix_coefficients, map_transfer_characteristics};
use crate::error::CodecError;

/// AV1 decoder wrapping dav1d.
///
/// Expects packets containing raw AV1 OBU data (as stored in MP4/WebM containers).
/// Unlike H.264/H.265, AV1 does not require separate parameter set initialization —
/// the sequence header is embedded in the bitstream.
pub struct Av1Decoder {
    inner: dav1d::Decoder,
    /// Buffered decoded frame (send/receive pattern: one frame per packet).
    pending_frame: Option<VideoFrame>,
    /// Whether end-of-stream has been signaled.
    flushing: bool,
}

impl Av1Decoder {
    /// Creates a new AV1 decoder.
    ///
    /// The `_av1c_data` parameter is the raw av1C box body from the container.
    /// dav1d does not require explicit initialization from av1C — the sequence
    /// header OBU in the bitstream is sufficient. The parameter is accepted for
    /// API consistency with H.264/H.265 decoders.
    pub fn new(_av1c_data: &[u8]) -> Result<Self, CodecError> {
        let inner = dav1d::Decoder::new().map_err(|e| CodecError::DecoderError {
            message: format!("failed to create dav1d decoder: {e}"),
        })?;

        Ok(Self {
            inner,
            pending_frame: None,
            flushing: false,
        })
    }

    /// Extracts color space from a decoded dav1d Picture.
    fn extract_color_space(picture: &dav1d::Picture) -> Option<ColorSpace> {
        let primaries = map_color_primaries(picture.color_primaries().to_u8()?)?;
        let transfer = map_transfer_characteristics(picture.transfer_characteristic().to_u8()?)?;
        let matrix = map_matrix_coefficients(picture.matrix_coefficients().to_u8()?)?;
        let range = match picture.color_range() {
            dav1d::pixel::YUVRange::Full => ColorRange::Full,
            dav1d::pixel::YUVRange::Limited => ColorRange::Limited,
        };

        Some(ColorSpace {
            primaries,
            transfer,
            matrix,
            range,
        })
    }

    /// Converts a dav1d Picture into a splica `VideoFrame`.
    fn picture_to_video_frame(
        picture: &dav1d::Picture,
        pts: splica_core::Timestamp,
    ) -> Result<VideoFrame, CodecError> {
        let w = picture.width();
        let h = picture.height();

        let pixel_format = match picture.pixel_layout() {
            dav1d::PixelLayout::I420 => PixelFormat::Yuv420p,
            other => {
                return Err(CodecError::Unsupported {
                    message: format!("unsupported AV1 pixel layout: {other:?}"),
                });
            }
        };

        let y_plane = picture.plane(PlanarImageComponent::Y);
        let u_plane = picture.plane(PlanarImageComponent::U);
        let v_plane = picture.plane(PlanarImageComponent::V);

        let y_stride = picture.stride(PlanarImageComponent::Y) as usize;
        let u_stride = picture.stride(PlanarImageComponent::U) as usize;

        // Chroma dimensions for 4:2:0
        let uvw = w.div_ceil(2);
        let uvh = h.div_ceil(2);

        let y_plane_size = y_stride * h as usize;
        let u_plane_size = u_stride * uvh as usize;
        let v_plane_size = u_stride * uvh as usize;
        let total_size = y_plane_size + u_plane_size + v_plane_size;

        let mut buf = Vec::with_capacity(total_size);
        buf.extend_from_slice(&y_plane[..y_plane_size]);
        buf.extend_from_slice(&u_plane[..u_plane_size]);
        buf.extend_from_slice(&v_plane[..v_plane_size]);

        let y_offset = 0;
        let u_offset = y_plane_size;
        let v_offset = y_plane_size + u_plane_size;

        let color_space = Self::extract_color_space(picture);

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
                stride: u_stride,
                width: uvw,
                height: uvh,
            },
        ];

        VideoFrame::new(
            w,
            h,
            pixel_format,
            color_space,
            pts,
            Bytes::from(buf),
            planes,
        )
        .map_err(|e| CodecError::DecoderError {
            message: format!("failed to create VideoFrame: {e}"),
        })
    }

    /// Tries to retrieve a decoded picture from dav1d.
    fn try_get_picture(&mut self, pts: splica_core::Timestamp) -> Result<(), CodecError> {
        match self.inner.get_picture() {
            Ok(picture) => {
                let frame = Self::picture_to_video_frame(&picture, pts)?;
                self.pending_frame = Some(frame);
            }
            Err(dav1d::Error::Again) => {
                self.pending_frame = None;
            }
            Err(e) => {
                return Err(CodecError::DecoderError {
                    message: format!("dav1d get_picture error: {e}"),
                });
            }
        }
        Ok(())
    }
}

impl Decoder for Av1Decoder {
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError> {
        match packet {
            Some(pkt) => {
                let pts_us = (pkt.pts.as_seconds_f64() * 1_000_000.0) as i64;

                // Send data to dav1d. If it returns Again, drain pictures until accepted.
                match self
                    .inner
                    .send_data(pkt.data.to_vec(), None, Some(pts_us), None)
                {
                    Ok(()) => {}
                    Err(dav1d::Error::Again) => {
                        // Drain pictures until send_pending_data succeeds
                        loop {
                            self.try_get_picture(pkt.pts)?;
                            match self.inner.send_pending_data() {
                                Ok(()) => break,
                                Err(dav1d::Error::Again) => continue,
                                Err(e) => {
                                    return Err(CodecError::DecoderError {
                                        message: format!("dav1d send_pending_data error: {e}"),
                                    }
                                    .into());
                                }
                            }
                        }
                    }
                    Err(e) => {
                        return Err(CodecError::DecoderError {
                            message: format!("dav1d send_data error: {e}"),
                        }
                        .into());
                    }
                }

                // Try to get a decoded picture
                self.try_get_picture(pkt.pts)?;
            }
            None => {
                // End of stream — flush
                self.flushing = true;
                self.inner.flush();

                // Try to get any remaining pictures
                let pts = splica_core::Timestamp::new(0, 1).unwrap();
                self.try_get_picture(pts)?;
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
