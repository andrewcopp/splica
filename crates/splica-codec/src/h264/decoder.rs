//! H.264 decoder implementation using OpenH264.
//!
//! All `unsafe` code (via OpenH264 FFI) is contained within the `openh264` crate.
//! This module uses only the safe Rust API provided by that crate.

use std::any::Any;
use std::collections::VecDeque;

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
    /// Buffered decoded frames (B-frame reordering may produce multiple
    /// frames per decode cycle, and flush may return several at once).
    pending_frames: VecDeque<VideoFrame>,
    /// Whether end-of-stream has been signaled.
    flushing: bool,
    /// Color space parsed from the SPS VUI parameters.
    color_space: Option<ColorSpace>,
    /// PTS of the last emitted frame, used to assign monotonic timestamps
    /// to flushed frames that the decoder holds in its lookahead buffer.
    last_emitted_pts: Option<splica_core::Timestamp>,
    /// Estimated frame duration, derived from the difference between the
    /// two most recent emitted PTS values. Falls back to 1 tick if unknown.
    frame_duration: Option<splica_core::Timestamp>,
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
            pending_frames: VecDeque::new(),
            flushing: false,
            color_space,
            last_emitted_pts: None,
            frame_duration: None,
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
                        self.pending_frames.push_back(frame);

                        // Track frame duration from consecutive PTS values
                        if let Some(prev_pts) = self.last_emitted_pts {
                            self.frame_duration = pkt.pts.checked_sub(prev_pts);
                        }
                        self.last_emitted_pts = Some(pkt.pts);
                    }
                    Ok(None) => {
                        // No frame produced yet (buffering, B-frame reordering, etc.)
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
                        let mut next_pts = self.last_emitted_pts;

                        for yuv in &frames {
                            // Compute the PTS for this flushed frame by advancing
                            // from the last emitted PTS by one frame duration.
                            let pts = match (next_pts, self.frame_duration) {
                                (Some(prev), Some(dur)) => prev.checked_add(dur),
                                (Some(prev), None) => {
                                    // No duration estimate — advance by 1 tick
                                    let one_tick = splica_core::Timestamp::new(1, prev.timebase());
                                    one_tick.and_then(|t| prev.checked_add(t))
                                }
                                (None, _) => None,
                            };

                            let pts = pts.unwrap_or_else(|| {
                                // Fallback: no frames were emitted before flush
                                // SAFETY: timebase 1 is always valid
                                splica_core::Timestamp::new(0, 1).expect("timebase 1 is non-zero")
                            });

                            let frame = Self::yuv_to_video_frame(yuv, pts, self.color_space)?;
                            self.pending_frames.push_back(frame);
                            next_pts = Some(pts);
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
        Ok(self.pending_frames.pop_front().map(Frame::Video))
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
    use splica_core::media::{PixelFormat, PlaneLayout, TrackIndex};
    use splica_core::Decoder;

    /// Parses Annex B byte stream into individual NAL units (without start codes).
    fn parse_annex_b_nals(data: &[u8]) -> Vec<Vec<u8>> {
        let mut nals = Vec::new();
        let mut i = 0;

        while i < data.len() {
            // Find start code (00 00 00 01 or 00 00 01)
            if i + 3 < data.len() && data[i] == 0 && data[i + 1] == 0 {
                let start;
                if data[i + 2] == 1 {
                    start = i + 3;
                } else if i + 4 <= data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                    start = i + 4;
                } else {
                    i += 1;
                    continue;
                }

                // Find the next start code
                let mut end = start;
                while end < data.len() {
                    if end + 3 < data.len()
                        && data[end] == 0
                        && data[end + 1] == 0
                        && (data[end + 2] == 1
                            || (end + 4 <= data.len() && data[end + 2] == 0 && data[end + 3] == 1))
                    {
                        break;
                    }
                    end += 1;
                }

                if start < end {
                    nals.push(data[start..end].to_vec());
                }
                i = end;
            } else {
                i += 1;
            }
        }

        nals
    }

    /// Converts Annex B NAL units to MP4 length-prefixed format (4-byte length).
    fn annex_b_to_mp4(annex_b: &[u8]) -> Vec<u8> {
        let nals = parse_annex_b_nals(annex_b);
        let mut out = Vec::new();
        for nal in &nals {
            let nal_type = nal[0] & 0x1F;
            // Skip SPS (7) and PPS (8) — they go in the avcC, not in sample data
            if nal_type == 7 || nal_type == 8 {
                continue;
            }
            let len = nal.len() as u32;
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(nal);
        }
        out
    }

    /// Builds a minimal avcC record from Annex B encoded data containing SPS/PPS.
    fn build_avcc_from_annex_b(annex_b: &[u8]) -> Vec<u8> {
        let nals = parse_annex_b_nals(annex_b);
        let mut sps_list = Vec::new();
        let mut pps_list = Vec::new();

        for nal in &nals {
            let nal_type = nal[0] & 0x1F;
            match nal_type {
                7 => sps_list.push(nal.clone()),
                8 => pps_list.push(nal.clone()),
                _ => {}
            }
        }

        let sps = sps_list.first().expect("no SPS found in encoded data");

        let profile_idc = if sps.len() > 1 { sps[1] } else { 0x42 };
        let compatibility = if sps.len() > 2 { sps[2] } else { 0xC0 };
        let level_idc = if sps.len() > 3 { sps[3] } else { 0x1E };

        let mut avcc = vec![
            1, // version
            profile_idc,
            compatibility,
            level_idc,
            0xFF, // length_size_minus_one = 3 (4 bytes), reserved bits set
            0xE0 | (sps_list.len() as u8), // num_sps with reserved bits
        ];

        for sps_nal in &sps_list {
            let len = sps_nal.len() as u16;
            avcc.extend_from_slice(&len.to_be_bytes());
            avcc.extend_from_slice(sps_nal);
        }

        avcc.push(pps_list.len() as u8);
        for pps_nal in &pps_list {
            let len = pps_nal.len() as u16;
            avcc.extend_from_slice(&len.to_be_bytes());
            avcc.extend_from_slice(pps_nal);
        }

        avcc
    }

    fn make_test_frame(width: u32, height: u32, pts_ticks: i64) -> VideoFrame {
        let y_stride = width as usize;
        let uv_stride = (width / 2) as usize;
        let y_size = y_stride * height as usize;
        let uv_size = uv_stride * (height / 2) as usize;

        let mut data = vec![128u8; y_size + uv_size * 2];
        // Vary Y plane slightly per frame to avoid skip frames
        let luma = (128_i64 + (pts_ticks % 10)) as u8;
        for b in &mut data[..y_size] {
            *b = luma;
        }

        VideoFrame::new(
            width,
            height,
            PixelFormat::Yuv420p,
            Some(splica_core::media::ColorSpace::BT709),
            splica_core::Timestamp::new(pts_ticks, 30000).unwrap(),
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
    fn test_that_flush_pts_values_are_monotonically_increasing() {
        use crate::h264::encoder::H264Encoder;
        use splica_core::Encoder;

        // GIVEN — encode several frames to produce valid H.264 data
        let mut encoder = H264Encoder::new().unwrap();
        let frame_count = 10;
        let mut encoded_packets = Vec::new();

        for i in 0..frame_count {
            let frame = make_test_frame(128, 128, i * 1001);
            encoder.send_frame(Some(&Frame::Video(frame))).unwrap();
            if let Some(pkt) = encoder.receive_packet().unwrap() {
                encoded_packets.push(pkt);
            }
        }

        // Flush encoder
        encoder.send_frame(None).unwrap();
        while let Some(pkt) = encoder.receive_packet().unwrap() {
            encoded_packets.push(pkt);
        }

        assert!(
            !encoded_packets.is_empty(),
            "encoder must produce at least one packet"
        );

        // Build avcC from the first packet (which should contain SPS/PPS as IDR)
        let avcc_data = build_avcc_from_annex_b(&encoded_packets[0].data);

        // WHEN — decode all packets then flush
        let mut decoder = H264Decoder::new(&avcc_data).unwrap();
        let mut all_pts = Vec::new();

        for pkt in &encoded_packets {
            let mp4_data = annex_b_to_mp4(&pkt.data);
            if mp4_data.is_empty() {
                continue;
            }
            let decode_packet = Packet {
                track_index: TrackIndex(0),
                pts: pkt.pts,
                dts: pkt.dts,
                is_keyframe: pkt.is_keyframe,
                data: Bytes::from(mp4_data),
            };
            decoder.send_packet(Some(&decode_packet)).unwrap();
            while let Some(frame) = decoder.receive_frame().unwrap() {
                all_pts.push(frame.pts());
            }
        }

        // Flush the decoder
        decoder.send_packet(None).unwrap();
        while let Some(frame) = decoder.receive_frame().unwrap() {
            all_pts.push(frame.pts());
        }

        // THEN — all PTS values are strictly monotonically increasing
        assert!(
            all_pts.len() >= 2,
            "need at least 2 frames to verify monotonicity, got {}",
            all_pts.len()
        );

        for window in all_pts.windows(2) {
            assert!(
                window[0] < window[1],
                "PTS must be monotonically increasing: {:?} should be < {:?}",
                window[0],
                window[1]
            );
        }
    }
}
