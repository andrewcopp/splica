//! Opus encoder using libopus (FFI).
//!
//! Wraps the `opus` crate behind the `codec-opus` feature flag.
//! Uses FFI to the reference Opus encoder library.

#[cfg(feature = "codec-opus")]
mod encoder;
#[cfg(feature = "codec-opus")]
pub use encoder::{OpusEncoder, OpusEncoderBuilder, OpusEncoderConfig};
