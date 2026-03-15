//! WebCodecs codec string builders.
//!
//! Re-exports shared implementations from `splica-core::codec_strings`.
//! These are used by WASM bindings to create `VideoDecoderConfig` objects.

pub(crate) use splica_core::codec_strings::{
    build_av1_codec_string, build_avc_codec_string, build_hevc_codec_string,
    extract_aac_audio_object_type,
};
