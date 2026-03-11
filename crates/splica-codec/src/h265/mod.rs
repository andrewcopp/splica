//! H.265 (HEVC) decoder using libde265.
//!
//! Wraps the `libde265` crate behind the `codec-h265` feature flag.
//! All FFI interaction with libde265 is confined to this module.

pub mod hvcc;

#[cfg(feature = "codec-h265")]
mod decoder;
#[cfg(feature = "codec-h265")]
pub use decoder::{H265Decoder, H265DecoderConfig};
