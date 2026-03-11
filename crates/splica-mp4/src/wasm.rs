//! WASM bindings for the MP4 demuxer.
//!
//! Provides a JS-callable API for extracting track metadata and reading
//! compressed packets from MP4 files. Enabled via the `wasm` feature flag.

use std::io::Cursor;

use wasm_bindgen::prelude::*;

use splica_core::wasm_types::{
    audio_track_info_json, video_track_info_json, WasmVideoDecoderConfig, WasmVideoPacket,
};
use splica_core::{Demuxer, SeekMode, Seekable, Timestamp, TrackKind};

use crate::boxes::stsd::CodecConfig;
use crate::codec_strings::{
    build_av1_codec_string, build_avc_codec_string, build_hevc_codec_string,
};
use crate::Mp4Demuxer;

/// MP4 demuxer accessible from JavaScript.
///
/// # Example (JS)
///
/// ```js
/// const response = await fetch("video.mp4");
/// const buffer = new Uint8Array(await response.arrayBuffer());
/// const demuxer = WasmMp4Demuxer.fromBytes(buffer);
/// console.log("Tracks:", demuxer.trackCount());
///
/// // Get WebCodecs-compatible decoder config
/// const config = demuxer.videoDecoderConfig();
/// if (config) {
///     const { codec, description } = JSON.parse(config);
///     const decoder = new VideoDecoder({ ... });
///     decoder.configure({ codec, description: new Uint8Array(description) });
///
///     // Feed packets to decoder
///     while (true) {
///         const packet = demuxer.readVideoPacket();
///         if (!packet) break;
///         const { data, timestamp_us, is_keyframe } = JSON.parse(packet);
///         decoder.decode(new EncodedVideoChunk({
///             type: is_keyframe ? 'key' : 'delta',
///             timestamp: timestamp_us,
///             data: new Uint8Array(data),
///         }));
///     }
/// }
/// ```
#[wasm_bindgen]
pub struct WasmMp4Demuxer {
    inner: Mp4Demuxer<Cursor<Vec<u8>>>,
}

#[wasm_bindgen]
impl WasmMp4Demuxer {
    /// Constructs an MP4 demuxer from an in-memory buffer.
    #[wasm_bindgen(js_name = "fromBytes")]
    pub fn from_bytes(data: &[u8]) -> Result<WasmMp4Demuxer, JsValue> {
        let cursor = Cursor::new(data.to_vec());
        let inner = Mp4Demuxer::open(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(WasmMp4Demuxer { inner })
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

    /// Returns a WebCodecs-compatible `VideoDecoderConfig`, or null if no
    /// video track is present.
    ///
    /// Supports H.264, H.265, and AV1 video tracks. Returns an error if the
    /// video track uses an unsupported or unknown codec.
    ///
    /// The returned config contains:
    /// - `codec`: WebCodecs codec string (e.g., `"avc1.42c01e"`)
    /// - `coded_width` / `coded_height`: video dimensions
    /// - `description`: raw codec config bytes for `VideoDecoderConfig.description`
    #[wasm_bindgen(js_name = "videoDecoderConfig")]
    pub fn video_decoder_config(&self) -> Result<Option<WasmVideoDecoderConfig>, JsValue> {
        let video_track = self
            .inner
            .tracks()
            .iter()
            .find(|t| t.kind == TrackKind::Video);

        let track = match video_track {
            Some(t) => t,
            None => return Ok(None),
        };

        let config = self.inner.codec_config(track.index);
        match config {
            Some(CodecConfig::Avc1 {
                avcc,
                width,
                height,
                ..
            }) => {
                let codec_string = build_avc_codec_string(avcc);
                Ok(Some(WasmVideoDecoderConfig::new(
                    codec_string,
                    *width as u32,
                    *height as u32,
                    avcc.to_vec(),
                )))
            }
            Some(CodecConfig::Hev1 {
                hvcc,
                width,
                height,
                ..
            }) => {
                let codec_string = build_hevc_codec_string(hvcc);
                Ok(Some(WasmVideoDecoderConfig::new(
                    codec_string,
                    *width as u32,
                    *height as u32,
                    hvcc.to_vec(),
                )))
            }
            Some(CodecConfig::Av1 {
                av1c,
                width,
                height,
                ..
            }) => {
                let codec_string = build_av1_codec_string(av1c);
                Ok(Some(WasmVideoDecoderConfig::new(
                    codec_string,
                    *width as u32,
                    *height as u32,
                    av1c.to_vec(),
                )))
            }
            Some(CodecConfig::Mp4a { .. }) => {
                // Audio-only codec config on a video track — shouldn't happen
                Err(JsValue::from_str(
                    "video track has audio codec config (mp4a)",
                ))
            }
            Some(CodecConfig::Unknown(name)) => Err(JsValue::from_str(&format!(
                "unsupported video codec: {name}"
            ))),
            None => Err(JsValue::from_str("video track has no codec configuration")),
        }
    }

    /// Seeks to the given timestamp in microseconds (keyframe mode).
    ///
    /// After seeking, subsequent `readVideoPacket()` / `nextPacket()` calls
    /// will return packets starting from the nearest keyframe at or before the
    /// target. Returns the actual seek position in microseconds, or an error.
    #[wasm_bindgen(js_name = "seekToTimestamp")]
    pub fn seek_to_timestamp(&mut self, timestamp_us: f64) -> Result<f64, JsValue> {
        let target = Timestamp::from_seconds(timestamp_us / 1_000_000.0, 1_000_000_000)
            .ok_or_else(|| JsValue::from_str("invalid seek timestamp"))?;

        self.inner
            .seek(target, SeekMode::Keyframe)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let actual_us = self
            .inner
            .seek_position()
            .map(|t| t.as_seconds_f64() * 1_000_000.0)
            .unwrap_or(timestamp_us);

        Ok(actual_us)
    }

    /// Reads the next video packet, skipping audio packets.
    ///
    /// Returns a `WasmVideoPacket` with the compressed data, presentation
    /// timestamp in microseconds, and keyframe flag. Returns null at
    /// end-of-stream.
    #[wasm_bindgen(js_name = "readVideoPacket")]
    pub fn read_video_packet(&mut self) -> Result<Option<WasmVideoPacket>, JsValue> {
        let video_index = self
            .inner
            .tracks()
            .iter()
            .find(|t| t.kind == TrackKind::Video)
            .map(|t| t.index);

        let video_index = match video_index {
            Some(idx) => idx,
            None => return Ok(None),
        };

        loop {
            match self.inner.read_packet() {
                Ok(Some(packet)) => {
                    if packet.track_index == video_index {
                        let timestamp_us = packet.pts.as_seconds_f64() * 1_000_000.0;
                        return Ok(Some(WasmVideoPacket::new(
                            packet.data.to_vec(),
                            timestamp_us,
                            packet.is_keyframe,
                        )));
                    }
                    // Skip non-video packets
                }
                Ok(None) => return Ok(None),
                Err(e) => return Err(JsValue::from_str(&e.to_string())),
            }
        }
    }
}
