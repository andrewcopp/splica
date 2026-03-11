//! Shared JS-facing types for WASM bindings.
//!
//! Used by `splica-mp4` and `splica-webm` wasm modules to avoid duplicating
//! track metadata structs and codec formatting logic.

use serde::Serialize;

use crate::media::{AudioCodec, Codec, TrackInfo, TrackKind, VideoCodec};

/// JS-facing video track metadata.
#[derive(Serialize)]
pub struct JsVideoTrackInfo {
    pub width: u32,
    pub height: u32,
    pub codec: String,
    pub frame_rate: Option<String>,
    pub duration_seconds: Option<f64>,
}

/// JS-facing audio track metadata.
#[derive(Serialize)]
pub struct JsAudioTrackInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: Option<u32>,
    pub duration_seconds: Option<f64>,
}

impl JsVideoTrackInfo {
    /// Builds from a `TrackInfo` that has `kind == TrackKind::Video`.
    pub fn from_track_info(track: &TrackInfo) -> Self {
        let video = track.video.as_ref();
        Self {
            width: video.map(|v| v.width).unwrap_or(0),
            height: video.map(|v| v.height).unwrap_or(0),
            codec: format_codec(&track.codec),
            frame_rate: video.and_then(|v| v.frame_rate.map(|fr| fr.to_string())),
            duration_seconds: track.duration.map(|d| d.as_seconds_f64()),
        }
    }
}

impl JsAudioTrackInfo {
    /// Builds from a `TrackInfo` that has `kind == TrackKind::Audio`.
    pub fn from_track_info(track: &TrackInfo) -> Self {
        let audio = track.audio.as_ref();
        Self {
            codec: format_codec(&track.codec),
            sample_rate: audio.map(|a| a.sample_rate).unwrap_or(0),
            channels: audio.and_then(|a| a.channel_layout.map(|cl| cl.channel_count())),
            duration_seconds: track.duration.map(|d| d.as_seconds_f64()),
        }
    }
}

/// Formats a `Codec` as a human-readable string for JS consumers.
pub fn format_codec(codec: &Codec) -> String {
    match codec {
        Codec::Video(vc) => match vc {
            VideoCodec::H264 => "H.264".to_string(),
            VideoCodec::H265 => "H.265".to_string(),
            VideoCodec::Av1 => "AV1".to_string(),
            VideoCodec::Other(s) => s.clone(),
        },
        Codec::Audio(ac) => match ac {
            AudioCodec::Aac => "AAC".to_string(),
            AudioCodec::Opus => "Opus".to_string(),
            AudioCodec::Other(s) => s.clone(),
        },
    }
}

/// Finds the first video track in a slice and returns its JSON representation.
pub fn video_track_info_json(tracks: &[TrackInfo]) -> Result<Option<String>, serde_json::Error> {
    let track = tracks.iter().find(|t| t.kind == TrackKind::Video);
    match track {
        Some(t) => serde_json::to_string(&JsVideoTrackInfo::from_track_info(t)).map(Some),
        None => Ok(None),
    }
}

/// Finds the first audio track in a slice and returns its JSON representation.
pub fn audio_track_info_json(tracks: &[TrackInfo]) -> Result<Option<String>, serde_json::Error> {
    let track = tracks.iter().find(|t| t.kind == TrackKind::Audio);
    match track {
        Some(t) => serde_json::to_string(&JsAudioTrackInfo::from_track_info(t)).map(Some),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// WASM-bindgen types (shared between container crates)
// ---------------------------------------------------------------------------

#[cfg(feature = "wasm")]
mod wasm_js_types {
    use wasm_bindgen::prelude::*;

    /// A compressed video packet with metadata for WebCodecs `EncodedVideoChunk`.
    #[wasm_bindgen]
    pub struct WasmVideoPacket {
        data: Vec<u8>,
        timestamp_us: f64,
        is_keyframe: bool,
    }

    impl WasmVideoPacket {
        /// Creates a new `WasmVideoPacket` from raw fields.
        pub fn new(data: Vec<u8>, timestamp_us: f64, is_keyframe: bool) -> Self {
            Self {
                data,
                timestamp_us,
                is_keyframe,
            }
        }
    }

    #[wasm_bindgen]
    impl WasmVideoPacket {
        /// The compressed video data.
        #[wasm_bindgen(getter)]
        pub fn data(&self) -> js_sys::Uint8Array {
            js_sys::Uint8Array::from(self.data.as_slice())
        }

        /// Presentation timestamp in microseconds.
        #[wasm_bindgen(getter, js_name = "timestampUs")]
        pub fn timestamp_us(&self) -> f64 {
            self.timestamp_us
        }

        /// Whether this packet is a keyframe.
        #[wasm_bindgen(getter, js_name = "isKeyframe")]
        pub fn is_keyframe(&self) -> bool {
            self.is_keyframe
        }
    }

    /// WebCodecs-compatible video decoder configuration.
    #[wasm_bindgen]
    pub struct WasmVideoDecoderConfig {
        codec: String,
        coded_width: u32,
        coded_height: u32,
        description: Vec<u8>,
    }

    impl WasmVideoDecoderConfig {
        /// Creates a new `WasmVideoDecoderConfig` from raw fields.
        pub fn new(
            codec: String,
            coded_width: u32,
            coded_height: u32,
            description: Vec<u8>,
        ) -> Self {
            Self {
                codec,
                coded_width,
                coded_height,
                description,
            }
        }
    }

    #[wasm_bindgen]
    impl WasmVideoDecoderConfig {
        /// WebCodecs codec string (e.g., `"avc1.42c01e"`, `"vp09.00.10.08"`).
        #[wasm_bindgen(getter)]
        pub fn codec(&self) -> String {
            self.codec.clone()
        }

        /// Coded video width.
        #[wasm_bindgen(getter, js_name = "codedWidth")]
        pub fn coded_width(&self) -> u32 {
            self.coded_width
        }

        /// Coded video height.
        #[wasm_bindgen(getter, js_name = "codedHeight")]
        pub fn coded_height(&self) -> u32 {
            self.coded_height
        }

        /// Raw codec-specific data for `VideoDecoderConfig.description`.
        #[wasm_bindgen(getter)]
        pub fn description(&self) -> js_sys::Uint8Array {
            js_sys::Uint8Array::from(self.description.as_slice())
        }
    }
}

#[cfg(feature = "wasm")]
pub use wasm_js_types::{WasmVideoDecoderConfig, WasmVideoPacket};
