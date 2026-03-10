//! WASM bindings for the WebM demuxer.
//!
//! Provides a JS-callable API for extracting track metadata and reading
//! compressed packets from WebM files. Enabled via the `wasm` feature flag.

use std::io::Cursor;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use splica_core::Demuxer;

use crate::WebmDemuxer;

/// JS-facing video track metadata.
#[derive(Serialize)]
struct JsVideoTrackInfo {
    width: u32,
    height: u32,
    codec: String,
    frame_rate: Option<String>,
    duration_seconds: Option<f64>,
}

/// JS-facing audio track metadata.
#[derive(Serialize)]
struct JsAudioTrackInfo {
    codec: String,
    sample_rate: u32,
    channels: Option<u32>,
    duration_seconds: Option<f64>,
}

/// WebM demuxer accessible from JavaScript.
///
/// # Example (JS)
///
/// ```js
/// const response = await fetch("video.webm");
/// const buffer = new Uint8Array(await response.arrayBuffer());
/// const demuxer = WasmWebmDemuxer.fromBytes(buffer);
/// console.log("Tracks:", demuxer.trackCount());
/// console.log("Video:", demuxer.videoTrackInfo());
/// while (true) {
///     const packet = demuxer.nextPacket();
///     if (!packet) break;
///     // process compressed packet bytes
/// }
/// ```
#[wasm_bindgen]
pub struct WasmWebmDemuxer {
    inner: WebmDemuxer<Cursor<Vec<u8>>>,
}

#[wasm_bindgen]
impl WasmWebmDemuxer {
    /// Constructs a WebM demuxer from an in-memory buffer.
    #[wasm_bindgen(js_name = "fromBytes")]
    pub fn from_bytes(data: &[u8]) -> Result<WasmWebmDemuxer, JsValue> {
        let cursor = Cursor::new(data.to_vec());
        let inner = WebmDemuxer::open(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(WasmWebmDemuxer { inner })
    }

    /// Returns the number of tracks in the container.
    #[wasm_bindgen(js_name = "trackCount")]
    pub fn track_count(&self) -> usize {
        self.inner.tracks().len()
    }

    /// Returns video track metadata as a JSON string, or null if no video track.
    #[wasm_bindgen(js_name = "videoTrackInfo")]
    pub fn video_track_info(&self) -> Result<JsValue, JsValue> {
        let video_track = self
            .inner
            .tracks()
            .iter()
            .find(|t| t.kind == splica_core::TrackKind::Video);

        match video_track {
            Some(t) => {
                let video = t.video.as_ref();
                let info = JsVideoTrackInfo {
                    width: video.map(|v| v.width).unwrap_or(0),
                    height: video.map(|v| v.height).unwrap_or(0),
                    codec: format_codec(&t.codec),
                    frame_rate: video.and_then(|v| v.frame_rate.map(|fr| fr.to_string())),
                    duration_seconds: t.duration.map(|d| d.as_seconds_f64()),
                };
                let json =
                    serde_json::to_string(&info).map_err(|e| JsValue::from_str(&e.to_string()))?;
                Ok(JsValue::from_str(&json))
            }
            None => Ok(JsValue::NULL),
        }
    }

    /// Returns audio track metadata as a JSON string, or null if no audio track.
    #[wasm_bindgen(js_name = "audioTrackInfo")]
    pub fn audio_track_info(&self) -> Result<JsValue, JsValue> {
        let audio_track = self
            .inner
            .tracks()
            .iter()
            .find(|t| t.kind == splica_core::TrackKind::Audio);

        match audio_track {
            Some(t) => {
                let audio = t.audio.as_ref();
                let info = JsAudioTrackInfo {
                    codec: format_codec(&t.codec),
                    sample_rate: audio.map(|a| a.sample_rate).unwrap_or(0),
                    channels: audio.and_then(|a| a.channel_layout.map(|cl| cl.channel_count())),
                    duration_seconds: t.duration.map(|d| d.as_seconds_f64()),
                };
                let json =
                    serde_json::to_string(&info).map_err(|e| JsValue::from_str(&e.to_string()))?;
                Ok(JsValue::from_str(&json))
            }
            None => Ok(JsValue::NULL),
        }
    }

    /// Returns the next compressed packet as bytes, or null at end-of-stream.
    #[wasm_bindgen(js_name = "nextPacket")]
    pub fn next_packet(&mut self) -> Result<JsValue, JsValue> {
        match self.inner.read_packet() {
            Ok(Some(packet)) => Ok(js_sys::Uint8Array::from(packet.data.as_ref()).into()),
            Ok(None) => Ok(JsValue::NULL),
            Err(e) => Err(JsValue::from_str(&e.to_string())),
        }
    }
}

fn format_codec(codec: &splica_core::Codec) -> String {
    match codec {
        splica_core::Codec::Video(vc) => match vc {
            splica_core::VideoCodec::H264 => "H.264".to_string(),
            splica_core::VideoCodec::H265 => "H.265".to_string(),
            splica_core::VideoCodec::Av1 => "AV1".to_string(),
            splica_core::VideoCodec::Other(s) => s.clone(),
        },
        splica_core::Codec::Audio(ac) => match ac {
            splica_core::AudioCodec::Aac => "AAC".to_string(),
            splica_core::AudioCodec::Opus => "Opus".to_string(),
            splica_core::AudioCodec::Other(s) => s.clone(),
        },
    }
}
