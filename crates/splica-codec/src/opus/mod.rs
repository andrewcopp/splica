//! Opus codec using libopus (FFI).
//!
//! Wraps the `opus` crate behind the `codec-opus` feature flag.
//! Uses FFI to the reference Opus encoder and decoder libraries.

#[cfg(feature = "codec-opus")]
mod decoder;
#[cfg(feature = "codec-opus")]
mod encoder;
#[cfg(feature = "codec-opus")]
pub use decoder::{OpusDecoder, OpusDecoderConfig};
#[cfg(feature = "codec-opus")]
pub use encoder::{OpusEncoder, OpusEncoderBuilder, OpusEncoderConfig};
