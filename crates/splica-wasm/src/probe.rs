//! Lightweight container probe for WASM consumers.
//!
//! Reuses existing demuxers via `Cursor<Vec<u8>>` to extract track metadata
//! without requiring a full streaming demux session.

use std::io::Cursor;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use splica_core::container_detect;
use splica_core::media::{ContainerFormat, TrackInfo, TrackKind};
use splica_core::traits::Demuxer;

#[derive(Serialize)]
struct ProbeResult {
    container: String,
    duration_ms: Option<f64>,
    tracks: Vec<ProbeTrack>,
    partial: bool,
}

#[derive(Serialize)]
struct ProbeTrack {
    index: u32,
    kind: String,
    codec: String,
    duration_ms: Option<f64>,
    width: Option<u32>,
    height: Option<u32>,
    frame_rate: Option<String>,
    sample_rate: Option<u32>,
    channels: Option<u32>,
}

fn build_track(t: &TrackInfo) -> ProbeTrack {
    ProbeTrack {
        index: t.index.0,
        kind: match t.kind {
            TrackKind::Video => "video",
            TrackKind::Audio => "audio",
            TrackKind::Subtitle => "subtitle",
        }
        .to_string(),
        codec: t.codec.to_string(),
        duration_ms: t.duration.map(|d| d.as_seconds_f64() * 1000.0),
        width: t.video.as_ref().map(|v| v.width),
        height: t.video.as_ref().map(|v| v.height),
        frame_rate: t
            .video
            .as_ref()
            .and_then(|v| v.frame_rate.map(|fr| fr.to_string())),
        sample_rate: t.audio.as_ref().map(|a| a.sample_rate),
        channels: t
            .audio
            .as_ref()
            .and_then(|a| a.channel_layout.map(|cl| cl.channel_count())),
    }
}

fn probe_inner(data: &[u8]) -> Result<ProbeResult, String> {
    let format = container_detect::detect_container(data)
        .ok_or("unrecognized container format — expected MP4, WebM, or MKV")?;

    let container = match format {
        ContainerFormat::Mp4 => "mp4",
        ContainerFormat::WebM => "webm",
        ContainerFormat::Mkv => "mkv",
    }
    .to_string();

    let cursor = Cursor::new(data.to_vec());

    let tracks_result: Result<Vec<TrackInfo>, _> = match format {
        ContainerFormat::Mp4 => splica_mp4::Mp4Demuxer::open(cursor)
            .map(|d| d.tracks().to_vec())
            .map_err(|e| e.to_string()),
        ContainerFormat::WebM => splica_webm::WebmDemuxer::open(cursor)
            .map(|d| d.tracks().to_vec())
            .map_err(|e| e.to_string()),
        ContainerFormat::Mkv => splica_mkv::MkvDemuxer::open(cursor)
            .map(|d| d.tracks().to_vec())
            .map_err(|e| e.to_string()),
    };

    match tracks_result {
        Ok(tracks) => {
            let probe_tracks: Vec<ProbeTrack> = tracks.iter().map(build_track).collect();
            let duration_ms = probe_tracks
                .iter()
                .filter_map(|t| t.duration_ms)
                .fold(None, |acc: Option<f64>, d| {
                    Some(acc.map_or(d, |a| a.max(d)))
                });
            Ok(ProbeResult {
                container,
                duration_ms,
                tracks: probe_tracks,
                partial: false,
            })
        }
        Err(_) => Ok(ProbeResult {
            container,
            duration_ms: None,
            tracks: Vec::new(),
            partial: true,
        }),
    }
}

/// Probes a media file header and returns track metadata as a JSON string.
///
/// Pass at least the first several kilobytes of a media file. The function
/// reuses the full demuxer parsers internally — for MP4 files, the `moov` box
/// must be present in the provided data for track info to be extracted.
///
/// Returns a JSON object with `container`, `duration_ms`, `tracks`, and
/// `partial` fields. If the demuxer cannot fully parse the header (e.g.,
/// MP4 with moov at end-of-file), `partial` is `true` and `tracks` is empty.
#[wasm_bindgen(js_name = "probeContainerHeader")]
pub fn probe_container_header(data: &[u8]) -> Result<String, JsError> {
    let result = probe_inner(data).map_err(|e| JsError::new(&e))?;
    serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> Vec<u8> {
        let path = format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR").replace("/crates/splica-wasm", "")
        );
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
    }

    #[test]
    fn test_that_probe_extracts_video_track_from_mp4() {
        let data = load_fixture("bigbuckbunny_h264.mp4");

        let result = probe_inner(&data).unwrap();

        assert_eq!(result.container, "mp4");
        assert!(!result.partial);
        let video = result.tracks.iter().find(|t| t.kind == "video").unwrap();
        assert!(video.width.unwrap() > 0);
        assert!(video.height.unwrap() > 0);
    }

    #[test]
    fn test_that_probe_extracts_tracks_from_webm() {
        let data = load_fixture("bigbuckbunny_vp9.webm");

        let result = probe_inner(&data).unwrap();

        assert_eq!(result.container, "webm");
        assert!(!result.partial);
        assert!(result.tracks.iter().any(|t| t.kind == "video"));
    }

    #[test]
    fn test_that_probe_extracts_tracks_from_mkv() {
        let data = load_fixture("bigbuckbunny_h264.mkv");

        let result = probe_inner(&data).unwrap();

        assert_eq!(result.container, "mkv");
        assert!(!result.partial);
        assert!(!result.tracks.is_empty());
    }

    #[test]
    fn test_that_probe_returns_error_for_unrecognized_format() {
        let data = vec![0xFF; 64];

        let result = probe_inner(&data);

        assert!(result.is_err());
    }

    #[test]
    fn test_that_probe_returns_partial_for_truncated_mp4() {
        // GIVEN — first 64 bytes: ftyp box only, no moov
        let full = load_fixture("bigbuckbunny_h264.mp4");
        let data = &full[..64];

        let result = probe_inner(data).unwrap();

        assert_eq!(result.container, "mp4");
        assert!(result.partial);
        assert!(result.tracks.is_empty());
    }
}
