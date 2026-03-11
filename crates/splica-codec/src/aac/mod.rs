//! AAC codec implementations.
//!
//! Decoder: symphonia (pure Rust, WASM-compatible) behind `codec-aac`.
//! Encoder: fdk-aac (FFI) behind `codec-aac-enc`.

#[cfg(feature = "codec-aac")]
mod decoder;
#[cfg(feature = "codec-aac")]
pub use decoder::{AacDecoder, AacDecoderConfig};

#[cfg(feature = "codec-aac-enc")]
mod encoder;
#[cfg(feature = "codec-aac-enc")]
pub use encoder::{AacEncoder, AacEncoderBuilder, AacEncoderConfig};
