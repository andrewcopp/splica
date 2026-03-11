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
