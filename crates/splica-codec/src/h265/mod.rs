//! H.265 (HEVC) codec support.
//!
//! - Decoder: wraps `libde265` behind the `decode-h265` feature flag.
//! - Encoder: wraps `kvazaar` behind the `encode-h265` feature flag.

pub mod hvcc;

#[cfg(feature = "decode-h265")]
mod decoder;
#[cfg(feature = "decode-h265")]
pub use decoder::{H265Decoder, H265DecoderConfig};

#[cfg(feature = "encode-h265")]
mod encoder;
#[cfg(feature = "encode-h265")]
pub use encoder::{H265Encoder, H265EncoderBuilder, H265EncoderConfig};
