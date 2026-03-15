//! AV1 codec support.
//!
//! Decode via `dav1d` (C FFI) gated behind `decode-av1`,
//! encode via `rav1e` (pure Rust) gated behind `encode-av1`.

#[cfg(feature = "decode-av1")]
mod decoder;
#[cfg(feature = "decode-av1")]
pub use decoder::Av1Decoder;

#[cfg(feature = "encode-av1")]
pub mod encoder;
#[cfg(feature = "encode-av1")]
pub use encoder::{Av1Encoder, Av1EncoderBuilder, Av1EncoderConfig};
