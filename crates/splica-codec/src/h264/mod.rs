//! H.264 (AVC) decoder and encoder using OpenH264.
//!
//! Wraps the `openh264` crate behind the `decode-h264` and `encode-h264` feature flags.
//! All FFI interaction with OpenH264 is confined to this module.

pub mod avcc;
pub(crate) mod sps;

#[cfg(feature = "decode-h264")]
mod decoder;
#[cfg(feature = "decode-h264")]
pub use decoder::{H264Decoder, H264DecoderConfig, H264Profile};

#[cfg(feature = "encode-h264")]
pub mod encoder;
#[cfg(feature = "encode-h264")]
pub use encoder::{
    H264Encoder, H264EncoderBuilder, H264EncoderConfig, H264EncoderLevel, H264EncoderProfile,
};
