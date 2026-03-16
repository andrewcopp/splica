//! Integration tests for subtitle track passthrough (SPL-176).
//!
//! Verifies that subtitle tracks survive stream copy operations instead of
//! being silently dropped. Uses synthetically-built MKV fixtures that contain
//! both a video track and a subtitle track.

use std::io::Cursor;
use std::process::Command;

use splica_core::{
    Codec, Muxer, Packet, SubtitleCodec, Timestamp, TrackIndex, TrackInfo, TrackKind, VideoCodec,
    VideoTrackInfo,
};
use splica_mkv::MkvMuxer;

fn splica_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_splica"))
}

fn test_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("splica_subtitle_tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn probe_json(path: &str) -> serde_json::Value {
    let output = splica_binary()
        .args(["probe", "--format", "json", path])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "probe should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap()
}

/// Builds a minimal MKV file with one H.264 video track and one subtitle track.
fn build_mkv_with_subtitle(subtitle_codec: SubtitleCodec) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = MkvMuxer::new(&mut output);

        // Track 0: video (H.264)
        let video_track = TrackInfo {
            index: TrackIndex(0),
            kind: TrackKind::Video,
            codec: Codec::Video(VideoCodec::H264),
            duration: None,
            video: Some(VideoTrackInfo {
                width: 320,
                height: 240,
                pixel_format: None,
                color_space: None,
                frame_rate: None,
                profile: None,
                level: None,
                color_primaries: None,
                transfer_characteristics: None,
                matrix_coefficients: None,
            }),
            audio: None,
        };
        muxer.add_track(&video_track).unwrap();

        // Track 1: subtitle
        let subtitle_track = TrackInfo {
            index: TrackIndex(1),
            kind: TrackKind::Subtitle,
            codec: Codec::Subtitle(subtitle_codec),
            duration: None,
            video: None,
            audio: None,
        };
        muxer.add_track(&subtitle_track).unwrap();

        // Write a fake video packet
        let video_packet = Packet {
            track_index: TrackIndex(0),
            pts: Timestamp::new(0, 1000).unwrap(),
            dts: Timestamp::new(0, 1000).unwrap(),
            is_keyframe: true,
            data: bytes::Bytes::from(vec![0u8; 100]),
        };
        muxer.write_packet(&video_packet).unwrap();

        // Write a subtitle packet
        let subtitle_data = b"1\n00:00:00,000 --> 00:00:01,000\nHello\n";
        let subtitle_packet = Packet {
            track_index: TrackIndex(1),
            pts: Timestamp::new(0, 1000).unwrap(),
            dts: Timestamp::new(0, 1000).unwrap(),
            is_keyframe: true,
            data: bytes::Bytes::from(subtitle_data.to_vec()),
        };
        muxer.write_packet(&subtitle_packet).unwrap();

        muxer.finalize().unwrap();
    }
    output.into_inner()
}

// ---------------------------------------------------------------------------
// Stream copy: SRT subtitle tracks should survive MKV → MKV
// ---------------------------------------------------------------------------

#[test]
fn test_that_stream_copy_preserves_srt_subtitle_track_in_mkv() {
    // GIVEN — an MKV file with a video track and an SRT subtitle track
    let dir = test_dir();
    let input_path = dir.join("input_srt_streamcopy.mkv");
    let output_path = dir.join("output_srt_streamcopy.mkv");

    let mkv_data = build_mkv_with_subtitle(SubtitleCodec::Srt);
    std::fs::write(&input_path, &mkv_data).unwrap();

    // WHEN — stream copy MKV → MKV (no re-encode flags)
    let output = splica_binary()
        .args([
            "process",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stream copy should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — probe output and verify a subtitle track is present
    let json = probe_json(output_path.to_str().unwrap());
    let tracks = json["tracks"].as_array().unwrap();
    let subtitle_tracks: Vec<&serde_json::Value> =
        tracks.iter().filter(|t| t["kind"] == "subtitle").collect();

    assert_eq!(
        subtitle_tracks.len(),
        1,
        "output should have exactly one subtitle track, got tracks: {tracks:?}"
    );
    assert!(
        subtitle_tracks[0]["codec"]
            .as_str()
            .unwrap()
            .contains("SRT"),
        "subtitle codec should be SRT, got: {}",
        subtitle_tracks[0]["codec"]
    );

    // Cleanup
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// Stream copy: WebVTT subtitle tracks should survive MKV → MKV
// ---------------------------------------------------------------------------

#[test]
fn test_that_stream_copy_preserves_webvtt_subtitle_track_in_mkv() {
    // GIVEN — an MKV file with a video track and a WebVTT subtitle track
    let dir = test_dir();
    let input_path = dir.join("input_webvtt_streamcopy.mkv");
    let output_path = dir.join("output_webvtt_streamcopy.mkv");

    let mkv_data = build_mkv_with_subtitle(SubtitleCodec::WebVtt);
    std::fs::write(&input_path, &mkv_data).unwrap();

    // WHEN — stream copy MKV → MKV
    let output = splica_binary()
        .args([
            "process",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stream copy should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — probe output and verify a WebVTT subtitle track is present
    let json = probe_json(output_path.to_str().unwrap());
    let tracks = json["tracks"].as_array().unwrap();
    let subtitle_tracks: Vec<&serde_json::Value> =
        tracks.iter().filter(|t| t["kind"] == "subtitle").collect();

    assert_eq!(
        subtitle_tracks.len(),
        1,
        "output should have exactly one subtitle track, got tracks: {tracks:?}"
    );
    assert!(
        subtitle_tracks[0]["codec"]
            .as_str()
            .unwrap()
            .contains("WebVTT"),
        "subtitle codec should be WebVTT, got: {}",
        subtitle_tracks[0]["codec"]
    );

    // Cleanup
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&output_path);
}
