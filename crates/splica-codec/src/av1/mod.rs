//! AV1 codec support.
//!
//! Decode via `dav1d` (C FFI) and encode via `rav1e` (pure Rust).
//! Both are gated behind the `codec-av1` feature flag.

#[cfg(feature = "codec-av1")]
mod decoder;
#[cfg(feature = "codec-av1")]
pub use decoder::Av1Decoder;

#[cfg(feature = "codec-av1")]
pub mod encoder;
#[cfg(feature = "codec-av1")]
pub use encoder::{Av1Encoder, Av1EncoderBuilder, Av1EncoderConfig};
