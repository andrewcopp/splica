//! AAC decoder using symphonia (pure Rust).
//!
//! Wraps the `symphonia-codec-aac` crate behind the `codec-aac` feature flag.
//! Pure Rust implementation — no FFI, WASM-compatible.

#[cfg(feature = "codec-aac")]
mod decoder;
#[cfg(feature = "codec-aac")]
pub use decoder::{AacDecoder, AacDecoderConfig};
