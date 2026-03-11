//! WASM bindings for the WebM demuxer.
//!
//! Provides a JS-callable API for extracting track metadata and reading
//! compressed packets from WebM files. Enabled via the `wasm` feature flag.

use std::io::Cursor;

use wasm_bindgen::prelude::*;

use splica_core::wasm_types::{audio_track_info_json, video_track_info_json};
use splica_core::Demuxer;

use crate::WebmDemuxer;

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
        match video_track_info_json(self.inner.tracks()) {
            Ok(Some(json)) => Ok(JsValue::from_str(&json)),
            Ok(None) => Ok(JsValue::NULL),
            Err(e) => Err(JsValue::from_str(&e.to_string())),
        }
    }

    /// Returns audio track metadata as a JSON string, or null if no audio track.
    #[wasm_bindgen(js_name = "audioTrackInfo")]
    pub fn audio_track_info(&self) -> Result<JsValue, JsValue> {
        match audio_track_info_json(self.inner.tracks()) {
            Ok(Some(json)) => Ok(JsValue::from_str(&json)),
            Ok(None) => Ok(JsValue::NULL),
            Err(e) => Err(JsValue::from_str(&e.to_string())),
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
