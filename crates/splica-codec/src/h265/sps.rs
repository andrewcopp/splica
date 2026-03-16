//! H.265 SPS/VUI parameter helpers for the kvazaar encoder.
//!
//! This module contains:
//! - ITU-T H.265 colour parameter mapping (VUI signaling)
//! - Kvazaar config option helpers
//! - Frame plane copy utilities

use std::ffi::CString;
use std::ptr;

use kvazaar_sys::{collect_chunks, kvz_api, kvz_config, kvz_data_chunk, kvz_encoder};
use splica_core::media::{
    ColorPrimaries, ColorRange, ColorSpace, MatrixCoefficients, TransferCharacteristics,
    VideoFrame,
};

use crate::error::CodecError;

/// Parameters for kvazaar encoder configuration.
pub(crate) struct KvzConfigParams<'a> {
    pub width: u32,
    pub height: u32,
    pub bitrate_bps: u32,
    pub qp: u8,
    pub max_frame_rate: Option<f32>,
    pub color_space: Option<&'a ColorSpace>,
}

/// Allocates, initializes, and configures a kvazaar config, then opens the
/// encoder and extracts the VPS/SPS/PPS header data.
///
/// Returns `(encoder_ptr, header_data)`. The caller owns the encoder and must
/// close it via `api.encoder_close`.
pub(crate) fn open_configured_encoder(
    api: &kvz_api,
    params: &KvzConfigParams<'_>,
) -> Result<(*mut kvz_encoder, Vec<u8>), CodecError> {
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

    apply_encoding_config(config_parse, cfg, params)?;

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

    let header_data = extract_header_data(api, encoder)?;
    Ok((encoder, header_data))
}

/// Applies all encoding options (dimensions, rate control, VUI) to a kvazaar config.
fn apply_encoding_config(
    config_parse: unsafe extern "C" fn(*mut kvz_config, *const i8, *const i8) -> i32,
    cfg: *mut kvz_config,
    params: &KvzConfigParams<'_>,
) -> Result<(), CodecError> {
    set_config_option(config_parse, cfg, "width", &params.width.to_string())?;
    set_config_option(config_parse, cfg, "height", &params.height.to_string())?;

    if params.bitrate_bps > 0 {
        set_config_option(config_parse, cfg, "bitrate", &params.bitrate_bps.to_string())?;
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

    if let Some(cs) = params.color_space {
        apply_vui_config(config_parse, cfg, cs)?;
    }

    Ok(())
}

/// Extracts VPS/SPS/PPS header data from an opened kvazaar encoder.
fn extract_header_data(api: &kvz_api, encoder: *mut kvz_encoder) -> Result<Vec<u8>, CodecError> {
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
    Ok(header_data)
}

/// Helper to call kvazaar `config_parse` and convert errors.
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

/// Applies VUI color space parameters to a kvazaar config.
///
/// Sets `colorprim`, `transfer`, `colormatrix`, and `range` options
/// based on the given `ColorSpace`.
pub(crate) fn apply_vui_config(
    config_parse: unsafe extern "C" fn(*mut kvz_config, *const i8, *const i8) -> i32,
    cfg: *mut kvz_config,
    cs: &ColorSpace,
) -> Result<(), CodecError> {
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

    Ok(())
}

/// Maps splica `ColorPrimaries` to ITU-T H.265 `colour_primaries` value.
fn color_primaries_to_itu(p: ColorPrimaries) -> u8 {
    match p {
        ColorPrimaries::Bt709 => 1,
        ColorPrimaries::Bt2020 => 9,
        ColorPrimaries::Smpte432 => 12,
    }
}

/// Maps splica `TransferCharacteristics` to ITU-T H.265 `transfer_characteristics` value.
fn transfer_characteristics_to_itu(t: TransferCharacteristics) -> u8 {
    match t {
        TransferCharacteristics::Bt709 => 1,
        TransferCharacteristics::Smpte2084 => 16,
        TransferCharacteristics::HybridLogGamma => 18,
    }
}

/// Maps splica `MatrixCoefficients` to ITU-T H.265 `matrix_coefficients` value.
fn matrix_coefficients_to_itu(m: MatrixCoefficients) -> u8 {
    match m {
        MatrixCoefficients::Identity => 0,
        MatrixCoefficients::Bt709 => 1,
        MatrixCoefficients::Bt2020NonConstant => 9,
        MatrixCoefficients::Bt2020Constant => 10,
    }
}

/// Copies one plane from a `VideoFrame` to a kvazaar picture buffer.
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
