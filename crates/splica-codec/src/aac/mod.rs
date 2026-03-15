//! AAC codec implementations.
//!
//! Decoder: symphonia (pure Rust, WASM-compatible) behind `decode-aac`.
//! Encoder: fdk-aac (FFI) behind `encode-aac`.

#[cfg(feature = "decode-aac")]
mod decoder;
#[cfg(feature = "decode-aac")]
pub use decoder::{AacDecoder, AacDecoderConfig};

#[cfg(feature = "encode-aac")]
mod encoder;
#[cfg(feature = "encode-aac")]
pub use encoder::{AacEncoder, AacEncoderBuilder, AacEncoderConfig};
