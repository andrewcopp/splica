//! FFI helper functions for the H.265 kvazaar encoder.
//!
//! Standalone utilities with no dependency on encoder state, extracted from
//! `encoder.rs` to keep file sizes manageable.

use std::ffi::CString;
use std::ptr;

use kvazaar_sys::{collect_chunks, kvz_api, kvz_api_get, kvz_config, kvz_data_chunk, kvz_encoder};
use splica_core::media::{
    ColorPrimaries, ColorRange, ColorSpace, MatrixCoefficients, TransferCharacteristics, VideoFrame,
};

use crate::error::CodecError;

/// Copies one plane from a VideoFrame to a kvazaar picture buffer.
///
/// # Safety
///
/// `dst` must point to a buffer with at least `dst_stride * height` bytes.
pub(crate) unsafe fn copy_plane(
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
pub(crate) fn set_config_option(
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
pub(crate) fn color_primaries_to_itu(p: ColorPrimaries) -> u8 {
    match p {
        ColorPrimaries::Bt709 => 1,
        ColorPrimaries::Bt2020 => 9,
        ColorPrimaries::Smpte432 => 12,
    }
}

/// Maps splica TransferCharacteristics to ITU-T H.265 transfer_characteristics value.
pub(crate) fn transfer_characteristics_to_itu(t: TransferCharacteristics) -> u8 {
    match t {
        TransferCharacteristics::Bt709 => 1,
        TransferCharacteristics::Smpte2084 => 16,
        TransferCharacteristics::HybridLogGamma => 18,
    }
}

/// Maps splica MatrixCoefficients to ITU-T H.265 matrix_coefficients value.
pub(crate) fn matrix_coefficients_to_itu(m: MatrixCoefficients) -> u8 {
    match m {
        MatrixCoefficients::Identity => 0,
        MatrixCoefficients::Bt709 => 1,
        MatrixCoefficients::Bt2020NonConstant => 9,
        MatrixCoefficients::Bt2020Constant => 10,
    }
}

/// Result of opening a kvazaar encoder via [`open_kvazaar_encoder`].
pub(crate) struct KvazaarEncoderHandle {
    pub api: &'static kvz_api,
    pub encoder: *mut kvz_encoder,
    pub header_data: Vec<u8>,
}

/// Encoder build parameters passed to [`open_kvazaar_encoder`].
pub(crate) struct KvazaarEncoderParams {
    pub width: u32,
    pub height: u32,
    pub bitrate_bps: u32,
    pub qp: u8,
    pub max_frame_rate: Option<f32>,
    pub color_space: Option<ColorSpace>,
}

/// Initializes the kvazaar API, configures and opens an encoder, and extracts
/// header data (VPS/SPS/PPS).
///
/// This is the FFI-heavy portion of `H265EncoderBuilder::build()`, extracted
/// to keep `encoder.rs` under the file size limit.
pub(crate) fn open_kvazaar_encoder(
    params: KvazaarEncoderParams,
) -> Result<KvazaarEncoderHandle, CodecError> {
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
    set_config_option(config_parse, cfg, "width", &params.width.to_string())?;
    set_config_option(config_parse, cfg, "height", &params.height.to_string())?;

    if params.bitrate_bps > 0 {
        set_config_option(
            config_parse,
            cfg,
            "bitrate",
            &params.bitrate_bps.to_string(),
        )?;
        set_config_option(config_parse, cfg, "rc-algorithm", "oba")?;
    } else {
        set_config_option(config_parse, cfg, "qp", &params.qp.to_string())?;
    }

    // Set intra period for keyframe interval (must be a multiple of B-gop length 16)
    set_config_option(config_parse, cfg, "period", "64")?;
    // Emit VPS/SPS/PPS with every intra frame
    set_config_option(config_parse, cfg, "vps-period", "1")?;

    if let Some(fps) = params.max_frame_rate {
        let fps_int = fps.round() as u32;
        if fps_int > 0 {
            set_config_option(config_parse, cfg, "input-fps", &fps_int.to_string())?;
        }
    }

    // Set VUI color space parameters
    if let Some(cs) = &params.color_space {
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

    Ok(KvazaarEncoderHandle {
        api,
        encoder,
        header_data,
    })
}
