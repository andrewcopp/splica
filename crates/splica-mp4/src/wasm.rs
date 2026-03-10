//! WASM bindings for the MP4 demuxer.
//!
//! Provides a minimal JS-callable API for extracting track metadata from MP4 files.
//! Enabled via the `wasm` feature flag.

use std::io::Cursor;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use splica_core::Demuxer;

use crate::Mp4Demuxer;

/// Track metadata returned to JS as JSON.
#[derive(Serialize)]
struct WasmTrackInfo {
    index: u32,
    kind: String,
    codec: String,
    duration_seconds: Option<f64>,
    width: Option<u32>,
    height: Option<u32>,
    sample_rate: Option<u32>,
}

/// Parses an MP4 file from a byte buffer and returns track metadata as JSON.
///
/// Accepts the full MP4 file contents as `&[u8]` (copied from JS `Uint8Array`).
/// Returns a JSON string describing each track's codec, dimensions, and duration.
///
/// # Example (JS)
///
/// ```js
/// const response = await fetch("video.mp4");
/// const buffer = new Uint8Array(await response.arrayBuffer());
/// const json = mp4_track_info(buffer);
/// const tracks = JSON.parse(json);
/// ```
#[wasm_bindgen]
pub fn mp4_track_info(data: &[u8]) -> Result<String, JsValue> {
    let demuxer =
        Mp4Demuxer::open(Cursor::new(data)).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let tracks: Vec<WasmTrackInfo> = demuxer
        .tracks()
        .iter()
        .map(|t| {
            let kind = match t.kind {
                splica_core::TrackKind::Video => "video",
                splica_core::TrackKind::Audio => "audio",
            };
            let codec = match &t.codec {
                splica_core::Codec::Video(vc) => format!("{vc:?}"),
                splica_core::Codec::Audio(ac) => format!("{ac:?}"),
            };
            WasmTrackInfo {
                index: t.index.0,
                kind: kind.to_string(),
                codec,
                duration_seconds: t.duration.map(|d| d.as_seconds_f64()),
                width: t.video.as_ref().map(|v| v.width),
                height: t.video.as_ref().map(|v| v.height),
                sample_rate: t.audio.as_ref().map(|a| a.sample_rate),
            }
        })
        .collect();

    serde_json::to_string(&tracks).map_err(|e| JsValue::from_str(&e.to_string()))
}
