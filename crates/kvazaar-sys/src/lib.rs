//! Raw FFI bindings for the kvazaar HEVC encoder (BSD-3-Clause).
//!
//! Bindings derived from `kvazaar.h` (v2.x). Only the API surface needed
//! by splica is exposed: encoder lifecycle, picture allocation, and data
//! chunk handling.
//!
//! The kvazaar C API uses a function-pointer table (`kvz_api`) returned by
//! `kvz_api_get()`. All encoder operations go through this table.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int};

/// Opaque encoder instance.
#[repr(C)]
pub struct kvz_encoder {
    _private: [u8; 0],
}

/// Opaque encoder configuration.
///
/// Allocated via `api.config_alloc()`, initialized via `api.config_init()`,
/// and individual options set via `api.config_parse()`. The struct layout
/// is intentionally opaque — all access goes through the API functions.
#[repr(C)]
pub struct kvz_config {
    _private: [u8; 0],
}

/// Data chunk size (must match `KVZ_DATA_CHUNK_SIZE` in kvazaar.h).
pub const KVZ_DATA_CHUNK_SIZE: usize = 4096;

/// Linked list node for encoded output data.
#[repr(C)]
pub struct kvz_data_chunk {
    pub data: [u8; KVZ_DATA_CHUNK_SIZE],
    pub len: u32,
    pub next: *mut kvz_data_chunk,
}

/// Chroma subsampling format.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum kvz_chroma_format {
    KVZ_CSP_400 = 0,
    KVZ_CSP_420 = 1,
    KVZ_CSP_422 = 2,
    KVZ_CSP_444 = 3,
}

/// Interlacing mode.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum kvz_interlacing {
    KVZ_INTERLACING_NONE = 0,
    KVZ_INTERLACING_TFF = 1,
    KVZ_INTERLACING_BFF = 2,
}

/// NAL unit type codes (Table 7-1, ITU-T H.265).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum kvz_nal_unit_type {
    KVZ_NAL_TRAIL_N = 0,
    KVZ_NAL_TRAIL_R = 1,
    KVZ_NAL_TSA_N = 2,
    KVZ_NAL_TSA_R = 3,
    KVZ_NAL_STSA_N = 4,
    KVZ_NAL_STSA_R = 5,
    KVZ_NAL_RADL_N = 6,
    KVZ_NAL_RADL_R = 7,
    KVZ_NAL_RASL_N = 8,
    KVZ_NAL_RASL_R = 9,
    KVZ_NAL_BLA_W_LP = 16,
    KVZ_NAL_BLA_W_RADL = 17,
    KVZ_NAL_BLA_N_LP = 18,
    KVZ_NAL_IDR_W_RADL = 19,
    KVZ_NAL_IDR_N_LP = 20,
    KVZ_NAL_CRA_NUT = 21,
    KVZ_NAL_VPS_NUT = 32,
    KVZ_NAL_SPS_NUT = 33,
    KVZ_NAL_PPS_NUT = 34,
    KVZ_NAL_AUD_NUT = 35,
    KVZ_NAL_EOS_NUT = 36,
    KVZ_NAL_EOB_NUT = 37,
    KVZ_NAL_FD_NUT = 38,
    KVZ_NAL_PREFIX_SEI_NUT = 39,
    KVZ_NAL_SUFFIX_SEI_NUT = 40,
}

/// Slice type.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum kvz_slice_type {
    KVZ_SLICE_B = 0,
    KVZ_SLICE_P = 1,
    KVZ_SLICE_I = 2,
}

/// Picture buffer allocated by kvazaar.
///
/// Fields before `width` are pointers managed by kvazaar — we only read
/// the y/u/v pointers to copy pixel data, and set pts for timestamp.
#[repr(C)]
pub struct kvz_picture {
    pub fulldata_buf: *mut u8,
    pub fulldata: *mut u8,
    pub y: *mut u8,
    pub u: *mut u8,
    pub v: *mut u8,
    pub data: [*mut u8; 3],
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub base_image: *mut kvz_picture,
    pub refcount: i32,
    pub pts: i64,
    pub dts: i64,
    pub interlacing: kvz_interlacing,
    pub chroma_format: kvz_chroma_format,
    pub ref_pocs: [i32; 16],
    pub roi: kvz_picture_roi,
}

/// ROI sub-struct within kvz_picture.
#[repr(C)]
pub struct kvz_picture_roi {
    pub width: c_int,
    pub height: c_int,
    pub roi_array: *mut i8,
}

/// Information about an encoded frame.
#[repr(C)]
pub struct kvz_frame_info {
    pub poc: i32,
    pub qp: i8,
    pub nal_unit_type: kvz_nal_unit_type,
    pub slice_type: kvz_slice_type,
    pub ref_list: [[c_int; 16]; 2],
    pub ref_list_len: [c_int; 2],
}

/// Function-pointer table for the kvazaar API.
///
/// Returned by `kvz_api_get()`. All function pointers are guaranteed non-null
/// when the API is obtained successfully.
#[repr(C)]
pub struct kvz_api {
    pub config_alloc: Option<unsafe extern "C" fn() -> *mut kvz_config>,

    pub config_destroy: Option<unsafe extern "C" fn(cfg: *mut kvz_config) -> c_int>,

    pub config_init: Option<unsafe extern "C" fn(cfg: *mut kvz_config) -> c_int>,

    pub config_parse: Option<
        unsafe extern "C" fn(
            cfg: *mut kvz_config,
            name: *const c_char,
            value: *const c_char,
        ) -> c_int,
    >,

    pub picture_alloc: Option<unsafe extern "C" fn(width: i32, height: i32) -> *mut kvz_picture>,

    pub picture_free: Option<unsafe extern "C" fn(pic: *mut kvz_picture)>,

    pub chunk_free: Option<unsafe extern "C" fn(chunk: *mut kvz_data_chunk)>,

    pub encoder_open: Option<unsafe extern "C" fn(cfg: *const kvz_config) -> *mut kvz_encoder>,

    pub encoder_close: Option<unsafe extern "C" fn(encoder: *mut kvz_encoder)>,

    pub encoder_headers: Option<
        unsafe extern "C" fn(
            encoder: *mut kvz_encoder,
            data_out: *mut *mut kvz_data_chunk,
            len_out: *mut u32,
        ) -> c_int,
    >,

    pub encoder_encode: Option<
        unsafe extern "C" fn(
            encoder: *mut kvz_encoder,
            pic_in: *mut kvz_picture,
            data_out: *mut *mut kvz_data_chunk,
            len_out: *mut u32,
            pic_out: *mut *mut kvz_picture,
            src_out: *mut *mut kvz_picture,
            info_out: *mut kvz_frame_info,
        ) -> c_int,
    >,

    pub picture_alloc_csp: Option<
        unsafe extern "C" fn(
            chroma_format: kvz_chroma_format,
            width: i32,
            height: i32,
        ) -> *mut kvz_picture,
    >,
}

extern "C" {
    /// Returns the kvazaar API function table for the given bit depth.
    ///
    /// Pass `8` for 8-bit encoding (the only depth splica uses).
    /// Returns null if the bit depth is not supported by this build of kvazaar.
    pub fn kvz_api_get(bit_depth: c_int) -> *const kvz_api;
}

/// Collects all data from a kvz_data_chunk linked list into a `Vec<u8>`.
///
/// # Safety
///
/// `chunk` must be a valid pointer returned by kvazaar's encoder_encode or
/// encoder_headers, or null.
pub unsafe fn collect_chunks(mut chunk: *mut kvz_data_chunk) -> Vec<u8> {
    let mut data = Vec::new();
    while !chunk.is_null() {
        // SAFETY: chunk is a valid kvz_data_chunk allocated by kvazaar.
        // We read `len` bytes from the `data` array, which is guaranteed
        // to be valid by kvazaar's contract.
        let c = unsafe { &*chunk };
        let len = c.len as usize;
        data.extend_from_slice(&c.data[..len]);
        chunk = c.next;
    }
    data
}
