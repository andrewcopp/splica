//! Codec trait implementations and FFI wrappers.
//!
//! # WASM codec strategy
//!
//! ## Decision: pure-Rust codecs for WASM, FFI for native
//!
//! WASM targets (`wasm32-unknown-unknown`) cannot use C FFI libraries like
//! `dav1d` or `fdk-aac` without Emscripten or complex wasm-ld setups. Instead:
//!
//! | Codec   | Native (FFI)        | WASM (pure Rust)     |
//! |---------|---------------------|----------------------|
//! | AV1 dec | `dav1d`             | `dav1d` via wasm-c   |
//! | AV1 enc | `rav1e`             | `rav1e` (pure Rust)  |
//! | H.264   | `openh264`          | `openh264` via wasm-c|
//! | AAC     | `fdk-aac`           | pure Rust (future)   |
//! | Opus    | `libopus`           | `opus-rs` / pure     |
//!
//! ### Rationale
//!
//! - **`rav1e`** is already pure Rust and compiles to WASM with no extra work.
//!   It is the default AV1 encoder on both native and WASM.
//! - **`dav1d`** (AV1 decode) and **`openh264`** (H.264) are C libraries.
//!   For WASM, they can be compiled via `wasm32-unknown-emscripten` or
//!   prebuilt `.wasm` modules linked at build time. This adds complexity
//!   but avoids writing pure-Rust decoders (which would be massive efforts).
//! - **AAC decode** has no good pure-Rust option today. For WASM, we can
//!   use the browser's built-in `AudioDecoder` API (WebCodecs) instead of
//!   shipping our own AAC decoder, keeping bundle size small.
//! - **Opus** has `opus-rs` (FFI) for native and can use WebCodecs for WASM.
//!
//! ### Feature flags
//!
//! Each codec direction is behind its own feature flag: `decode-h264`,
//! `encode-h264`, `decode-av1`, `encode-av1`, etc. Enable `all-codecs`
//! for everything. WASM builds enable the same flags but link different
//! implementations via `#[cfg(target_arch = "wasm32")]`.
//!
//! ### Bundle size implications
//!
//! Pure-Rust codecs (rav1e) add significant WASM size (~2-4MB). For
//! browser use cases that only need demuxing/remuxing (no transcode),
//! codec crates should not be linked — the `splica-mp4` demuxer alone
//! should compile to <100KB wasm.

pub mod aac;
pub mod av1;
pub(crate) mod color;
pub mod error;
pub mod h264;
pub mod h265;
pub mod opus;

#[cfg(feature = "decode-h264")]
pub use h264::{H264Decoder, H264DecoderConfig, H264Profile};

#[cfg(feature = "encode-h264")]
pub use h264::{
    H264Encoder, H264EncoderBuilder, H264EncoderConfig, H264EncoderLevel, H264EncoderProfile,
};

#[cfg(feature = "decode-h265")]
pub use h265::{H265Decoder, H265DecoderConfig};

#[cfg(feature = "encode-h265")]
pub use h265::{H265Encoder, H265EncoderBuilder, H265EncoderConfig};

#[cfg(feature = "decode-aac")]
pub use aac::{AacDecoder, AacDecoderConfig};

#[cfg(feature = "encode-aac")]
pub use aac::{AacEncoder, AacEncoderBuilder, AacEncoderConfig};

#[cfg(feature = "decode-opus")]
pub use opus::{OpusDecoder, OpusDecoderConfig};

#[cfg(feature = "encode-opus")]
pub use opus::{OpusEncoder, OpusEncoderBuilder, OpusEncoderConfig};

#[cfg(feature = "decode-av1")]
pub use av1::Av1Decoder;

#[cfg(feature = "encode-av1")]
pub use av1::{Av1Encoder, Av1EncoderBuilder, Av1EncoderConfig};
