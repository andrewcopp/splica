//! Shared JS-facing types for WASM bindings.
//!
//! Used by `splica-mp4` and `splica-webm` wasm modules to avoid duplicating
//! track metadata structs and codec formatting logic.

use serde::Serialize;

use crate::media::{TrackInfo, TrackKind};

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
            codec: track.codec.to_string(),
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
            codec: track.codec.to_string(),
            sample_rate: audio.map(|a| a.sample_rate).unwrap_or(0),
            channels: audio.and_then(|a| a.channel_layout.map(|cl| cl.channel_count())),
            duration_seconds: track.duration.map(|d| d.as_seconds_f64()),
        }
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

    /// A compressed audio packet with metadata for WebCodecs `EncodedAudioChunk`.
    #[wasm_bindgen]
    pub struct WasmAudioPacket {
        data: Vec<u8>,
        timestamp_us: f64,
        duration_us: f64,
        is_keyframe: bool,
    }

    impl WasmAudioPacket {
        /// Creates a new `WasmAudioPacket` from raw fields.
        ///
        /// Use `-1.0` for `duration_us` when the duration is unknown.
        pub fn new(data: Vec<u8>, timestamp_us: f64, duration_us: f64, is_keyframe: bool) -> Self {
            Self {
                data,
                timestamp_us,
                duration_us,
                is_keyframe,
            }
        }
    }

    #[wasm_bindgen]
    impl WasmAudioPacket {
        /// The compressed audio data.
        #[wasm_bindgen(getter)]
        pub fn data(&self) -> js_sys::Uint8Array {
            js_sys::Uint8Array::from(self.data.as_slice())
        }

        /// Presentation timestamp in microseconds.
        #[wasm_bindgen(getter, js_name = "timestampUs")]
        pub fn timestamp_us(&self) -> f64 {
            self.timestamp_us
        }

        /// Duration in microseconds, or -1 if unknown.
        #[wasm_bindgen(getter, js_name = "durationUs")]
        pub fn duration_us(&self) -> f64 {
            self.duration_us
        }

        /// Whether this packet is a keyframe.
        #[wasm_bindgen(getter, js_name = "isKeyframe")]
        pub fn is_keyframe(&self) -> bool {
            self.is_keyframe
        }
    }

    /// WebCodecs-compatible audio decoder configuration.
    #[wasm_bindgen]
    pub struct WasmAudioDecoderConfig {
        codec: String,
        description: Vec<u8>,
        sample_rate: u32,
        number_of_channels: u32,
    }

    impl WasmAudioDecoderConfig {
        /// Creates a new `WasmAudioDecoderConfig` from raw fields.
        pub fn new(
            codec: String,
            description: Vec<u8>,
            sample_rate: u32,
            number_of_channels: u32,
        ) -> Self {
            Self {
                codec,
                description,
                sample_rate,
                number_of_channels,
            }
        }
    }

    #[wasm_bindgen]
    impl WasmAudioDecoderConfig {
        /// WebCodecs codec string (e.g., `"mp4a.40.2"` for AAC-LC, `"opus"`).
        #[wasm_bindgen(getter)]
        pub fn codec(&self) -> String {
            self.codec.clone()
        }

        /// Raw codec-specific data for `AudioDecoderConfig.description`.
        ///
        /// For AAC this is the esds box contents; for Opus this is the OpusHead.
        #[wasm_bindgen(getter)]
        pub fn description(&self) -> js_sys::Uint8Array {
            js_sys::Uint8Array::from(self.description.as_slice())
        }

        /// Sample rate in Hz.
        #[wasm_bindgen(getter, js_name = "sampleRate")]
        pub fn sample_rate(&self) -> u32 {
            self.sample_rate
        }

        /// Number of audio channels.
        #[wasm_bindgen(getter, js_name = "numberOfChannels")]
        pub fn number_of_channels(&self) -> u32 {
            self.number_of_channels
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
pub use wasm_js_types::{
    WasmAudioDecoderConfig, WasmAudioPacket, WasmVideoDecoderConfig, WasmVideoPacket,
};
