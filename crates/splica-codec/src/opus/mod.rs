//! Opus codec using libopus (FFI).
//!
//! Wraps the `opus` crate behind the `decode-opus` and `encode-opus` feature flags.
//! Uses FFI to the reference Opus encoder and decoder libraries.

#[cfg(feature = "decode-opus")]
mod decoder;
#[cfg(feature = "decode-opus")]
pub use decoder::{OpusDecoder, OpusDecoderConfig};

#[cfg(feature = "encode-opus")]
mod encoder;
#[cfg(feature = "encode-opus")]
pub use encoder::{OpusEncoder, OpusEncoderBuilder, OpusEncoderConfig};
