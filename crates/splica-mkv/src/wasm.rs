//! WASM bindings for the MKV demuxer.
//!
//! Provides a JS-callable API for extracting track metadata and reading
//! compressed packets from MKV files. Enabled via the `wasm` feature flag.

use std::io::Cursor;

use wasm_bindgen::prelude::*;

use splica_core::wasm_types::{
    audio_track_info_json, video_track_info_json, WasmAudioDecoderConfig, WasmAudioPacket,
    WasmVideoDecoderConfig, WasmVideoPacket,
};
use splica_core::{
    AudioCodec, Codec, Demuxer, SeekMode, Seekable, Timestamp, TrackKind, VideoCodec,
};

use crate::MkvDemuxer;

/// MKV demuxer accessible from JavaScript.
///
/// # Example (JS)
///
/// ```js
/// const response = await fetch("video.mkv");
/// const buffer = new Uint8Array(await response.arrayBuffer());
/// const demuxer = WasmMkvDemuxer.fromBytes(buffer);
/// console.log("Tracks:", demuxer.trackCount());
/// console.log("Video:", demuxer.videoTrackInfo());
/// while (true) {
///     const packet = demuxer.nextPacket();
///     if (!packet) break;
///     // process compressed packet bytes
/// }
/// ```
#[wasm_bindgen]
pub struct WasmMkvDemuxer {
    inner: MkvDemuxer<Cursor<Vec<u8>>>,
}

#[wasm_bindgen]
impl WasmMkvDemuxer {
    /// Constructs an MKV demuxer from an in-memory buffer.
    #[wasm_bindgen(js_name = "fromBytes")]
    pub fn from_bytes(data: &[u8]) -> Result<WasmMkvDemuxer, JsValue> {
        let cursor = Cursor::new(data.to_vec());
        let inner = MkvDemuxer::open(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(WasmMkvDemuxer { inner })
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
    /// Supports H.264, H.265, VP8, VP9, and AV1 video tracks. For VP9, parses
    /// the CodecPrivate data to build an accurate codec string. Returns an error
    /// if the video track uses an unsupported codec.
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

        let video = match &track.video {
            Some(v) => v,
            None => return Ok(None),
        };

        let codec_private = self.inner.codec_private(track.index);
        match &track.codec {
            Codec::Video(VideoCodec::H264) => {
                // H.264 in MKV: CodecPrivate contains avcC config
                let description = codec_private.map(|d| d.to_vec()).unwrap_or_default();
                Ok(Some(WasmVideoDecoderConfig::new(
                    build_h264_codec_string(codec_private),
                    video.width,
                    video.height,
                    description,
                )))
            }
            Codec::Video(VideoCodec::H265) => {
                // H.265 in MKV: CodecPrivate contains hvcC config
                let description = codec_private.map(|d| d.to_vec()).unwrap_or_default();
                Ok(Some(WasmVideoDecoderConfig::new(
                    "hev1".to_string(),
                    video.width,
                    video.height,
                    description,
                )))
            }
            Codec::Video(VideoCodec::Av1) => {
                // AV1 in MKV: CodecPrivate contains av1C config
                let description = codec_private.map(|d| d.to_vec()).unwrap_or_default();
                Ok(Some(WasmVideoDecoderConfig::new(
                    "av01".to_string(),
                    video.width,
                    video.height,
                    description,
                )))
            }
            Codec::Video(VideoCodec::Other(name)) if name == "VP8" => {
                Ok(Some(WasmVideoDecoderConfig::new(
                    "vp8".to_string(),
                    video.width,
                    video.height,
                    Vec::new(),
                )))
            }
            Codec::Video(VideoCodec::Other(name)) if name == "VP9" => {
                let codec_string = build_vp9_codec_string(codec_private);
                Ok(Some(WasmVideoDecoderConfig::new(
                    codec_string,
                    video.width,
                    video.height,
                    Vec::new(),
                )))
            }
            Codec::Video(vc) => Err(JsValue::from_str(&format!(
                "unsupported video codec in MKV: {vc}"
            ))),
            Codec::Audio(_) => Err(JsValue::from_str(
                "video track has audio codec (unexpected)",
            )),
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
    /// Returns a `WasmVideoPacket` with compressed data, presentation
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
                }
                Ok(None) => return Ok(None),
                Err(e) => return Err(JsValue::from_str(&e.to_string())),
            }
        }
    }

    /// Reads the next audio packet, skipping video packets.
    ///
    /// Returns a `WasmAudioPacket` with compressed data, presentation
    /// timestamp in microseconds, duration, and keyframe flag. Returns null
    /// at end-of-stream or if no audio track is present.
    #[wasm_bindgen(js_name = "readAudioPacket")]
    pub fn read_audio_packet(&mut self) -> Result<Option<WasmAudioPacket>, JsValue> {
        let audio_index = self
            .inner
            .tracks()
            .iter()
            .find(|t| t.kind == TrackKind::Audio)
            .map(|t| t.index);

        let audio_index = match audio_index {
            Some(idx) => idx,
            None => return Ok(None),
        };

        loop {
            match self.inner.read_packet() {
                Ok(Some(packet)) => {
                    if packet.track_index == audio_index {
                        let timestamp_us = packet.pts.as_seconds_f64() * 1_000_000.0;
                        return Ok(Some(WasmAudioPacket::new(
                            packet.data.to_vec(),
                            timestamp_us,
                            -1.0,
                            packet.is_keyframe,
                        )));
                    }
                    // Skip non-audio packets
                }
                Ok(None) => return Ok(None),
                Err(e) => return Err(JsValue::from_str(&e.to_string())),
            }
        }
    }

    /// Returns a WebCodecs-compatible `AudioDecoderConfig`, or null if no
    /// audio track is present.
    ///
    /// Supports AAC and Opus audio tracks. Returns an error if the audio
    /// track uses an unsupported or unknown codec.
    ///
    /// The returned config contains:
    /// - `codec`: WebCodecs codec string (e.g., `"opus"`, `"mp4a.40.2"`)
    /// - `description`: raw CodecPrivate bytes for `AudioDecoderConfig.description`
    /// - `sampleRate`: audio sample rate in Hz
    /// - `numberOfChannels`: number of audio channels
    #[wasm_bindgen(js_name = "audioDecoderConfig")]
    pub fn audio_decoder_config(&self) -> Result<Option<WasmAudioDecoderConfig>, JsValue> {
        let audio_track = self
            .inner
            .tracks()
            .iter()
            .find(|t| t.kind == TrackKind::Audio);

        let track = match audio_track {
            Some(t) => t,
            None => return Ok(None),
        };

        let audio_info = match &track.audio {
            Some(a) => a,
            None => return Ok(None),
        };

        let codec_private = self.inner.codec_private(track.index);
        let channels = audio_info
            .channel_layout
            .map(|cl| cl.channel_count())
            .unwrap_or(1);

        match &track.codec {
            Codec::Audio(AudioCodec::Opus) => {
                let description = codec_private.map(|d| d.to_vec()).unwrap_or_default();
                Ok(Some(WasmAudioDecoderConfig::new(
                    "opus".to_string(),
                    description,
                    audio_info.sample_rate,
                    channels,
                )))
            }
            Codec::Audio(AudioCodec::Aac) => {
                let description = codec_private.map(|d| d.to_vec()).unwrap_or_default();
                Ok(Some(WasmAudioDecoderConfig::new(
                    "mp4a.40.2".to_string(),
                    description,
                    audio_info.sample_rate,
                    channels,
                )))
            }
            Codec::Audio(AudioCodec::Other(name)) => Err(JsValue::from_str(&format!(
                "unsupported audio codec in MKV: {name}"
            ))),
            Codec::Video(_) => Err(JsValue::from_str(
                "audio track has video codec (unexpected)",
            )),
        }
    }
}

/// Builds a WebCodecs H.264 codec string from CodecPrivate (avcC) data.
///
/// Extracts profile, constraint flags, and level from the avcC box to produce
/// a string like `"avc1.42c01e"`. Falls back to `"avc1"` when CodecPrivate
/// is absent or too short.
fn build_h264_codec_string(codec_private: Option<&[u8]>) -> String {
    if let Some(data) = codec_private {
        // avcC structure: version(1) + profile(1) + compat(1) + level(1) ...
        if data.len() >= 4 {
            let profile = data[1];
            let compat = data[2];
            let level = data[3];
            return format!("avc1.{profile:02x}{compat:02x}{level:02x}");
        }
    }
    "avc1".to_string()
}

/// Builds a WebCodecs VP9 codec string from CodecPrivate data.
///
/// Parses VP Codec ISO Media File Format features (profile, level, bit depth)
/// from the CodecPrivate bytes. Falls back to `"vp09.00.10.08"` (profile 0,
/// level 1.0, 8-bit) when CodecPrivate is absent or too short to parse.
fn build_vp9_codec_string(codec_private: Option<&[u8]>) -> String {
    let mut profile: u8 = 0;
    let mut level: u8 = 10;
    let mut bit_depth: u8 = 8;

    if let Some(data) = codec_private {
        // VP Codec ISO Media File Format: sequence of (id: u8, length: u8, value: [u8])
        let mut pos = 0;
        while pos + 2 <= data.len() {
            let id = data[pos];
            let len = data[pos + 1] as usize;
            pos += 2;
            if pos + len > data.len() {
                break;
            }
            if len == 1 {
                match id {
                    1 => profile = data[pos],
                    2 => level = data[pos],
                    3 => bit_depth = data[pos],
                    _ => {}
                }
            }
            pos += len;
        }
    }

    format!("vp09.{profile:02}.{level:02}.{bit_depth:02}")
}
