//! H.265 encoder implementation using kvazaar FFI.
//!
//! All `unsafe` code interfaces with kvazaar through `kvazaar-sys` bindings.
//! Every unsafe block has a `// SAFETY:` comment explaining the invariant.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::ffi::CString;
use std::ptr;

use bytes::Bytes;
use splica_core::error::EncodeError;
use splica_core::media::{
    ColorPrimaries, ColorRange, ColorSpace, Frame, MatrixCoefficients, Packet, PixelFormat,
    QualityTarget, TrackIndex, TransferCharacteristics, VideoFrame,
};
use splica_core::Encoder;

use kvazaar_sys::{
    collect_chunks, kvz_api, kvz_api_get, kvz_chroma_format, kvz_config, kvz_data_chunk,
    kvz_encoder, kvz_frame_info, kvz_nal_unit_type, kvz_picture,
};

use crate::error::CodecError;

/// H.265 encoder wrapping kvazaar.
///
/// Accepts YUV420p `VideoFrame`s and produces Annex B encoded H.265 packets.
/// Kvazaar uses a lookahead buffer so output packets may lag behind input frames.
pub struct H265Encoder {
    api: &'static kvz_api,
    encoder: *mut kvz_encoder,
    config: H265EncoderConfig,
    track_index: TrackIndex,
    /// Buffered encoded packets from the last send_frame call.
    pending_packets: VecDeque<Packet>,
    /// Map from picture order count (poc) to original input PTS.
    pts_by_poc: HashMap<i32, splica_core::Timestamp>,
    /// Last PTS assigned to an output packet, used as fallback during flush.
    last_pts: Option<splica_core::Timestamp>,
    /// Frame counter for PTS tracking.
    frame_count: u64,
    /// Whether end-of-stream has been signaled.
    flushing: bool,
    /// Annex B header data (VPS/SPS/PPS) from encoder_headers.
    header_data: Vec<u8>,
    /// Whether the header has been prepended to the first keyframe.
    header_sent: bool,
    /// Maximum frame rate hint for timestamp calculation.
    max_frame_rate: Option<f32>,
}

/// H.265 encoder configuration parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H265EncoderConfig {
    /// Target bitrate in bits per second (0 = QP mode).
    pub bitrate_bps: u32,
    /// Quantization parameter (0–51, used when bitrate_bps is 0).
    pub qp: u8,
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
}

/// Builder for creating an `H265Encoder` with specific settings.
pub struct H265EncoderBuilder {
    bitrate_bps: u32,
    qp: u8,
    width: u32,
    height: u32,
    track_index: TrackIndex,
    max_frame_rate: Option<f32>,
    color_space: Option<ColorSpace>,
}

impl H265EncoderBuilder {
    /// Creates a new encoder builder with default settings.
    ///
    /// Default is QP mode with QP=23 (visually transparent).
    pub fn new() -> Self {
        Self {
            bitrate_bps: 0,
            qp: QualityTarget::DEFAULT_CRF,
            width: 0,
            height: 0,
            track_index: TrackIndex(0),
            max_frame_rate: None,
            color_space: None,
        }
    }

    /// Sets encoder quality from a `QualityTarget`.
    ///
    /// - `Bitrate(bps)` → sets target bitrate with OBA rate control.
    /// - `Crf(crf)` → maps to kvazaar QP (0–51, same scale as HEVC QP).
    pub fn quality(mut self, target: QualityTarget) -> Self {
        match target {
            QualityTarget::Bitrate(bps) => {
                self.bitrate_bps = bps;
                self.qp = 0;
            }
            QualityTarget::Crf(crf) => {
                self.qp = crf.min(QualityTarget::MAX_CRF);
                self.bitrate_bps = 0;
            }
        }
        self
    }

    /// Sets the frame dimensions (required before build).
    pub fn dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
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

    /// Sets the color space for VUI signaling in the output VPS/SPS.
    pub fn color_space(mut self, cs: ColorSpace) -> Self {
        self.color_space = Some(cs);
        self
    }

    /// Builds the H.265 encoder.
    pub fn build(self) -> Result<H265Encoder, CodecError> {
        if self.width == 0 || self.height == 0 {
            return Err(CodecError::InvalidConfig {
                message: "H.265 encoder requires non-zero dimensions".to_string(),
            });
        }

        // SAFETY: kvz_api_get(8) returns a pointer to a static, immutable API
        // table for 8-bit encoding. The pointer is valid for the lifetime of
        // the process if non-null.
        let api = unsafe { kvz_api_get(8) };
        if api.is_null() {
            return Err(CodecError::EncoderError {
                message: "kvazaar does not support 8-bit encoding".to_string(),
            });
        }
        // SAFETY: api is non-null and points to a static kvz_api struct.
        let api = unsafe { &*api };

        // Allocate and initialize config
        let config_alloc = api.config_alloc.ok_or_else(|| CodecError::EncoderError {
            message: "kvazaar API missing config_alloc".to_string(),
        })?;
        let config_init = api.config_init.ok_or_else(|| CodecError::EncoderError {
            message: "kvazaar API missing config_init".to_string(),
        })?;
        let config_parse = api.config_parse.ok_or_else(|| CodecError::EncoderError {
            message: "kvazaar API missing config_parse".to_string(),
        })?;

        // SAFETY: config_alloc returns a heap-allocated kvz_config or null.
        let cfg = unsafe { config_alloc() };
        if cfg.is_null() {
            return Err(CodecError::EncoderError {
                message: "kvazaar config_alloc returned null".to_string(),
            });
        }
        // SAFETY: cfg is a valid, non-null kvz_config pointer from config_alloc.
        let ok = unsafe { config_init(cfg) };
        if ok == 0 {
            // SAFETY: cfg was allocated by config_alloc, safe to destroy.
            unsafe { api.config_destroy.map(|f| f(cfg)) };
            return Err(CodecError::EncoderError {
                message: "kvazaar config_init failed".to_string(),
            });
        }

        // Set dimensions, QP, and rate control via config_parse
        set_config_option(config_parse, cfg, "width", &self.width.to_string())?;
        set_config_option(config_parse, cfg, "height", &self.height.to_string())?;

        if self.bitrate_bps > 0 {
            set_config_option(config_parse, cfg, "bitrate", &self.bitrate_bps.to_string())?;
            set_config_option(config_parse, cfg, "rc-algorithm", "oba")?;
        } else {
            set_config_option(config_parse, cfg, "qp", &self.qp.to_string())?;
        }

        // Set intra period for keyframe interval (must be a multiple of B-gop length 16)
        set_config_option(config_parse, cfg, "period", "64")?;
        // Emit VPS/SPS/PPS with every intra frame
        set_config_option(config_parse, cfg, "vps-period", "1")?;

        if let Some(fps) = self.max_frame_rate {
            let fps_int = fps.round() as u32;
            if fps_int > 0 {
                set_config_option(config_parse, cfg, "input-fps", &fps_int.to_string())?;
            }
        }

        // Set VUI color space parameters
        if let Some(cs) = &self.color_space {
            let prim = color_primaries_to_itu(cs.primaries);
            let transfer = transfer_characteristics_to_itu(cs.transfer);
            let matrix = matrix_coefficients_to_itu(cs.matrix);
            let range = match cs.range {
                ColorRange::Full => "full",
                ColorRange::Limited => "limited",
            };

            set_config_option(config_parse, cfg, "colorprim", &prim.to_string())?;
            set_config_option(config_parse, cfg, "transfer", &transfer.to_string())?;
            set_config_option(config_parse, cfg, "colormatrix", &matrix.to_string())?;
            set_config_option(config_parse, cfg, "range", range)?;
        }

        // Open encoder
        let encoder_open = api.encoder_open.ok_or_else(|| CodecError::EncoderError {
            message: "kvazaar API missing encoder_open".to_string(),
        })?;
        // SAFETY: cfg is a valid, fully configured kvz_config pointer.
        let encoder = unsafe { encoder_open(cfg) };

        // Config is consumed by encoder_open; we no longer own it.
        // (kvazaar copies the config internally, but we don't destroy it
        // here — kvazaar manages its own copy.)
        // SAFETY: cfg was allocated by config_alloc. We destroy our copy now
        // since encoder_open has copied it.
        unsafe { api.config_destroy.map(|f| f(cfg)) };

        if encoder.is_null() {
            return Err(CodecError::EncoderError {
                message: "kvazaar encoder_open returned null".to_string(),
            });
        }

        // Get header data (VPS/SPS/PPS)
        let mut header_data = Vec::new();
        if let Some(encoder_headers) = api.encoder_headers {
            let mut data_out: *mut kvz_data_chunk = ptr::null_mut();
            let mut len_out: u32 = 0;
            // SAFETY: encoder is a valid, non-null kvz_encoder pointer.
            // data_out and len_out are valid mutable references.
            let ok = unsafe { encoder_headers(encoder, &mut data_out, &mut len_out) };
            if ok != 0 && !data_out.is_null() {
                // SAFETY: data_out is a valid kvz_data_chunk chain from encoder_headers.
                header_data = unsafe { collect_chunks(data_out) };
                // SAFETY: data_out was allocated by kvazaar; chunk_free releases it.
                if let Some(chunk_free) = api.chunk_free {
                    unsafe { chunk_free(data_out) };
                }
            }
        }

        Ok(H265Encoder {
            api,
            encoder,
            config: H265EncoderConfig {
                bitrate_bps: self.bitrate_bps,
                qp: self.qp,
                width: self.width,
                height: self.height,
            },
            track_index: self.track_index,
            pending_packets: VecDeque::new(),
            pts_by_poc: HashMap::new(),
            last_pts: None,
            frame_count: 0,
            flushing: false,
            header_data,
            header_sent: false,
            max_frame_rate: self.max_frame_rate,
        })
    }
}

impl Default for H265EncoderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl H265Encoder {
    /// Returns a builder for configuring the encoder.
    pub fn builder() -> H265EncoderBuilder {
        H265EncoderBuilder::new()
    }

    /// Returns the encoder configuration.
    pub fn encoder_config(&self) -> &H265EncoderConfig {
        &self.config
    }

    /// Encodes one frame or flushes, collecting output packets.
    fn encode_frame(&mut self, pic: *mut kvz_picture) -> Result<(), EncodeError> {
        let encoder_encode = self
            .api
            .encoder_encode
            .ok_or_else(|| CodecError::EncoderError {
                message: "kvazaar API missing encoder_encode".to_string(),
            })?;

        let mut data_out: *mut kvz_data_chunk = ptr::null_mut();
        let mut len_out: u32 = 0;
        let mut pic_out: *mut kvz_picture = ptr::null_mut();
        let mut src_out: *mut kvz_picture = ptr::null_mut();
        let mut info_out: kvz_frame_info = unsafe { std::mem::zeroed() };

        // SAFETY: encoder is a valid kvz_encoder pointer. pic is either a valid
        // kvz_picture pointer or null (for flush). All output pointers are valid.
        let ok = unsafe {
            encoder_encode(
                self.encoder,
                pic,
                &mut data_out,
                &mut len_out,
                &mut pic_out,
                &mut src_out,
                &mut info_out,
            )
        };

        if ok == 0 {
            return Err(CodecError::EncoderError {
                message: "kvazaar encoder_encode failed".to_string(),
            }
            .into());
        }

        // Free reconstructed and source pictures if returned
        if let Some(picture_free) = self.api.picture_free {
            if !pic_out.is_null() {
                // SAFETY: pic_out is a valid kvz_picture from encoder_encode.
                unsafe { picture_free(pic_out) };
            }
            if !src_out.is_null() {
                // SAFETY: src_out is a valid kvz_picture from encoder_encode.
                unsafe { picture_free(src_out) };
            }
        }

        if len_out > 0 && !data_out.is_null() {
            // SAFETY: data_out is a valid kvz_data_chunk chain from encoder_encode.
            let mut encoded_data = unsafe { collect_chunks(data_out) };
            // SAFETY: data_out was allocated by kvazaar; chunk_free releases it.
            if let Some(chunk_free) = self.api.chunk_free {
                unsafe { chunk_free(data_out) };
            }

            let is_keyframe = matches!(
                info_out.nal_unit_type,
                kvz_nal_unit_type::KVZ_NAL_IDR_W_RADL
                    | kvz_nal_unit_type::KVZ_NAL_IDR_N_LP
                    | kvz_nal_unit_type::KVZ_NAL_CRA_NUT
                    | kvz_nal_unit_type::KVZ_NAL_BLA_W_LP
                    | kvz_nal_unit_type::KVZ_NAL_BLA_W_RADL
                    | kvz_nal_unit_type::KVZ_NAL_BLA_N_LP
            );

            // Prepend VPS/SPS/PPS header to first keyframe
            if is_keyframe && !self.header_sent && !self.header_data.is_empty() {
                let mut with_header = self.header_data.clone();
                with_header.append(&mut encoded_data);
                encoded_data = with_header;
                self.header_sent = true;
            }

            // Look up the original input PTS by poc instead of reconstructing
            let pts = if let Some(original_pts) = self.pts_by_poc.remove(&info_out.poc) {
                self.last_pts = Some(original_pts);
                original_pts
            } else if let Some(last) = self.last_pts {
                // Fallback during flush: increment from last known PTS
                let next =
                    splica_core::Timestamp::new(last.ticks() + 1, last.timebase()).unwrap_or(last);
                self.last_pts = Some(next);
                next
            } else {
                splica_core::Timestamp::new(0, 1).unwrap()
            };

            let packet = Packet {
                track_index: self.track_index,
                pts,
                dts: pts,
                is_keyframe,
                data: Bytes::from(encoded_data),
            };

            self.pending_packets.push_back(packet);
        }

        Ok(())
    }
}

impl Encoder for H265Encoder {
    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<(), EncodeError> {
        self.pending_packets.clear();

        match frame {
            Some(Frame::Video(video_frame)) => {
                if video_frame.pixel_format != PixelFormat::Yuv420p {
                    return Err(EncodeError::InvalidFrame {
                        message: format!(
                            "H.265 encoder requires Yuv420p, got {:?}",
                            video_frame.pixel_format
                        ),
                    });
                }

                if video_frame.planes.len() != 3 {
                    return Err(EncodeError::InvalidFrame {
                        message: format!(
                            "H.265 encoder requires 3 planes, got {}",
                            video_frame.planes.len()
                        ),
                    });
                }

                let picture_alloc_csp =
                    self.api
                        .picture_alloc_csp
                        .ok_or_else(|| CodecError::EncoderError {
                            message: "kvazaar API missing picture_alloc_csp".to_string(),
                        })?;

                // SAFETY: picture_alloc_csp returns a heap-allocated kvz_picture
                // for the given chroma format and dimensions, or null.
                let pic = unsafe {
                    picture_alloc_csp(
                        kvz_chroma_format::KVZ_CSP_420,
                        self.config.width as i32,
                        self.config.height as i32,
                    )
                };
                if pic.is_null() {
                    return Err(CodecError::EncoderError {
                        message: "kvazaar picture_alloc_csp returned null".to_string(),
                    }
                    .into());
                }

                // Copy pixel data from VideoFrame to kvz_picture
                // SAFETY: pic is a valid, non-null kvz_picture with allocated
                // y/u/v planes matching the configured dimensions.
                unsafe {
                    let p = &mut *pic;
                    copy_plane(
                        video_frame,
                        0,
                        p.y,
                        p.stride as usize,
                        self.config.width as usize,
                        self.config.height as usize,
                    );
                    copy_plane(
                        video_frame,
                        1,
                        p.u,
                        p.stride as usize / 2,
                        self.config.width as usize / 2,
                        self.config.height as usize / 2,
                    );
                    copy_plane(
                        video_frame,
                        2,
                        p.v,
                        p.stride as usize / 2,
                        self.config.width as usize / 2,
                        self.config.height as usize / 2,
                    );

                    // Set PTS from input frame
                    p.pts = video_frame.pts.ticks();
                }

                self.pts_by_poc
                    .insert(self.frame_count as i32, video_frame.pts);
                let result = self.encode_frame(pic);

                // Free the input picture
                // SAFETY: pic was allocated by picture_alloc_csp and is no longer
                // referenced by kvazaar (encoder_encode copies what it needs).
                if let Some(picture_free) = self.api.picture_free {
                    unsafe { picture_free(pic) };
                }

                self.frame_count += 1;
                result
            }
            Some(Frame::Audio(_)) => Err(EncodeError::InvalidFrame {
                message: "H.265 encoder received audio frame".to_string(),
            }),
            None => {
                // Flush: send null frames until no more output
                self.flushing = true;
                loop {
                    let packet_count_before = self.pending_packets.len();
                    self.encode_frame(ptr::null_mut())?;
                    if self.pending_packets.len() == packet_count_before {
                        break;
                    }
                }
                Ok(())
            }
        }
    }

    fn receive_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending_packets.pop_front())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Drop for H265Encoder {
    fn drop(&mut self) {
        if let Some(encoder_close) = self.api.encoder_close {
            if !self.encoder.is_null() {
                // SAFETY: encoder is a valid kvz_encoder pointer from encoder_open.
                unsafe { encoder_close(self.encoder) };
            }
        }
    }
}

// SAFETY: The kvazaar encoder is thread-safe; all mutable state is internal
// to the C library and protected by its own synchronization. The kvz_api table
// is a static immutable struct. The encoder pointer is only used from &mut self.
unsafe impl Send for H265Encoder {}

/// Copies one plane from a VideoFrame to a kvazaar picture buffer.
///
/// # Safety
///
/// `dst` must point to a buffer with at least `dst_stride * height` bytes.
unsafe fn copy_plane(
    frame: &VideoFrame,
    plane_idx: usize,
    dst: *mut u8,
    dst_stride: usize,
    width: usize,
    height: usize,
) {
    let plane = &frame.planes[plane_idx];
    let src = &frame.data[plane.offset..];
    for row in 0..height {
        let src_row = &src[row * plane.stride..row * plane.stride + width];
        // SAFETY: dst is a valid kvazaar plane buffer. We write exactly
        // `width` bytes at offset `row * dst_stride`, which is within bounds
        // of the allocated plane.
        unsafe {
            ptr::copy_nonoverlapping(src_row.as_ptr(), dst.add(row * dst_stride), width);
        }
    }
}

/// Helper to call config_parse and convert errors.
fn set_config_option(
    config_parse: unsafe extern "C" fn(*mut kvz_config, *const i8, *const i8) -> i32,
    cfg: *mut kvz_config,
    name: &str,
    value: &str,
) -> Result<(), CodecError> {
    let c_name = CString::new(name).map_err(|_| CodecError::InvalidConfig {
        message: format!("invalid config name: {name}"),
    })?;
    let c_value = CString::new(value).map_err(|_| CodecError::InvalidConfig {
        message: format!("invalid config value: {value}"),
    })?;
    // SAFETY: cfg is a valid kvz_config pointer. c_name and c_value are
    // valid null-terminated C strings.
    let ok = unsafe { config_parse(cfg, c_name.as_ptr(), c_value.as_ptr()) };
    if ok == 0 {
        return Err(CodecError::InvalidConfig {
            message: format!("kvazaar rejected config {name}={value}"),
        });
    }
    Ok(())
}

/// Maps splica ColorPrimaries to ITU-T H.265 colour_primaries value.
fn color_primaries_to_itu(p: ColorPrimaries) -> u8 {
    match p {
        ColorPrimaries::Bt709 => 1,
        ColorPrimaries::Bt2020 => 9,
        ColorPrimaries::Smpte432 => 12,
    }
}

/// Maps splica TransferCharacteristics to ITU-T H.265 transfer_characteristics value.
fn transfer_characteristics_to_itu(t: TransferCharacteristics) -> u8 {
    match t {
        TransferCharacteristics::Bt709 => 1,
        TransferCharacteristics::Smpte2084 => 16,
        TransferCharacteristics::HybridLogGamma => 18,
    }
}

/// Maps splica MatrixCoefficients to ITU-T H.265 matrix_coefficients value.
fn matrix_coefficients_to_itu(m: MatrixCoefficients) -> u8 {
    match m {
        MatrixCoefficients::Identity => 0,
        MatrixCoefficients::Bt709 => 1,
        MatrixCoefficients::Bt2020NonConstant => 9,
        MatrixCoefficients::Bt2020Constant => 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use splica_core::media::PlaneLayout;
    use splica_core::Timestamp;

    /// Create a synthetic YUV420p VideoFrame with a solid color.
    fn make_test_frame(width: u32, height: u32, pts_ticks: i64) -> VideoFrame {
        let y_stride = width as usize;
        let uv_stride = (width / 2) as usize;
        let y_size = y_stride * height as usize;
        let uv_size = uv_stride * (height / 2) as usize;

        let mut data = vec![0u8; y_size + uv_size * 2];
        for b in &mut data[..y_size] {
            *b = 128;
        }
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
            Some(ColorSpace::BT709),
            Timestamp::new(pts_ticks, 30).unwrap(),
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
    fn test_that_h265_encoder_produces_packets_from_frames() {
        // GIVEN — an encoder and a synthetic video frame
        let mut encoder = H265EncoderBuilder::new()
            .dimensions(64, 64)
            .build()
            .unwrap();
        let frame = make_test_frame(64, 64, 0);

        // WHEN — send a frame, flush, and collect all packets
        encoder.send_frame(Some(&Frame::Video(frame))).unwrap();
        let mut packets = Vec::new();
        while let Some(pkt) = encoder.receive_packet().unwrap() {
            packets.push(pkt);
        }

        // Flush to get buffered frames
        encoder.send_frame(None).unwrap();
        while let Some(pkt) = encoder.receive_packet().unwrap() {
            packets.push(pkt);
        }

        // THEN — at least one non-empty packet is produced
        assert!(!packets.is_empty(), "expected at least one packet");
        assert!(
            !packets[0].data.is_empty(),
            "packet data should be non-empty"
        );
    }

    #[test]
    fn test_that_h265_encoder_produces_multiple_packets() {
        // GIVEN — an encoder
        let mut encoder = H265EncoderBuilder::new()
            .dimensions(64, 64)
            .build()
            .unwrap();

        // WHEN — encode 5 frames and flush
        let mut packets = Vec::new();
        for i in 0..5 {
            let frame = make_test_frame(64, 64, i);
            encoder.send_frame(Some(&Frame::Video(frame))).unwrap();
            while let Some(pkt) = encoder.receive_packet().unwrap() {
                packets.push(pkt);
            }
        }
        encoder.send_frame(None).unwrap();
        while let Some(pkt) = encoder.receive_packet().unwrap() {
            packets.push(pkt);
        }

        // THEN — 5 packets produced (one per input frame)
        assert_eq!(packets.len(), 5);
    }

    #[test]
    fn test_that_h265_encoder_rejects_non_yuv420p() {
        let mut encoder = H265EncoderBuilder::new()
            .dimensions(64, 64)
            .build()
            .unwrap();
        let y_size = 64 * 64;
        let uv_size = 32 * 64;
        let frame = VideoFrame::new(
            64,
            64,
            PixelFormat::Yuv422p,
            Some(ColorSpace::BT709),
            Timestamp::new(0, 30).unwrap(),
            Bytes::from(vec![0u8; y_size + uv_size * 2]),
            vec![
                PlaneLayout {
                    offset: 0,
                    stride: 64,
                    width: 64,
                    height: 64,
                },
                PlaneLayout {
                    offset: y_size,
                    stride: 32,
                    width: 32,
                    height: 64,
                },
                PlaneLayout {
                    offset: y_size + uv_size,
                    stride: 32,
                    width: 32,
                    height: 64,
                },
            ],
        )
        .unwrap();

        let result = encoder.send_frame(Some(&Frame::Video(frame)));
        assert!(result.is_err());
    }

    #[test]
    fn test_that_h265_encoder_requires_dimensions() {
        let result = H265EncoderBuilder::new().build();
        assert!(result.is_err());
    }

    #[test]
    fn test_that_h265_encoder_config_is_accessible() {
        let encoder = H265EncoderBuilder::new()
            .dimensions(128, 128)
            .quality(QualityTarget::Crf(28))
            .build()
            .unwrap();

        let config = encoder.encoder_config();
        assert_eq!(config.qp, 28);
        assert_eq!(config.width, 128);
        assert_eq!(config.height, 128);
    }
}
