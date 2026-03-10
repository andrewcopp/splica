//! H.264 (AVC) decoder using OpenH264.
//!
//! Wraps the `openh264` crate behind the `codec-h264` feature flag.
//! All FFI interaction with OpenH264 is confined to this module.

pub mod avcc;

#[cfg(feature = "codec-h264")]
mod decoder;
#[cfg(feature = "codec-h264")]
pub use decoder::{H264Decoder, H264DecoderConfig, H264Profile};

#[cfg(feature = "codec-h264")]
pub mod encoder;
#[cfg(feature = "codec-h264")]
pub use encoder::{
    H264Encoder, H264EncoderBuilder, H264EncoderConfig, H264EncoderLevel, H264EncoderProfile,
};
