//! H.265 (HEVC) codec support.
//!
//! - Decoder: wraps `libde265` behind the `codec-h265` feature flag.
//! - Encoder: wraps `kvazaar` behind the `codec-h265-enc` feature flag.

pub mod hvcc;

#[cfg(feature = "codec-h265")]
mod decoder;
#[cfg(feature = "codec-h265")]
pub use decoder::{H265Decoder, H265DecoderConfig};

#[cfg(feature = "codec-h265-enc")]
mod encoder;
#[cfg(feature = "codec-h265-enc")]
mod ffi_helpers;
#[cfg(feature = "codec-h265-enc")]
pub use encoder::{H265Encoder, H265EncoderBuilder, H265EncoderConfig};
