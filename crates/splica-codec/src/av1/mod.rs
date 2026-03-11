//! AV1 decoder using dav1d.
//!
//! Wraps the `dav1d` crate behind the `codec-av1` feature flag.
//! All FFI interaction with dav1d is confined to this module.

#[cfg(feature = "codec-av1")]
mod decoder;
#[cfg(feature = "codec-av1")]
pub use decoder::Av1Decoder;
