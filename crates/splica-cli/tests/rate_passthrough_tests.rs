//! Integration tests verifying frame rate and sample rate passthrough
//! through the decode-encode roundtrip (transcode path).
//!
//! Stream copy preserves rates by construction. The risky path is transcode,
//! where fractional frame rates (e.g. 29.97 fps) can silently drift or round.
//! These tests probe input and output and compare the reported rates.

use std::process::Command;

fn splica_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_splica"))
}

fn fixture_path(name: &str) -> String {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    workspace_root
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// Probe a file and return the parsed JSON.
fn probe_json(path: &str) -> serde_json::Value {
    let output = splica_binary()
        .args(["probe", "--format", "json", path])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "probe should succeed for {path}. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("expected valid JSON from probe of {path}, got error {e}: {stdout}")
    })
}

/// Extract the frame_rate string from the first video track in probe JSON.
fn video_frame_rate(json: &serde_json::Value) -> Option<String> {
    let tracks = json["tracks"].as_array()?;
    tracks
        .iter()
        .find(|t| t["kind"] == "video")
        .and_then(|t| t["frame_rate"].as_str())
        .map(|s| s.to_string())
}

/// Extract the sample_rate from the first audio track in probe JSON.
#[allow(dead_code)] // Kept for use when audio fixtures are added.
fn audio_sample_rate(json: &serde_json::Value) -> Option<u64> {
    let tracks = json["tracks"].as_array()?;
    tracks
        .iter()
        .find(|t| t["kind"] == "audio")
        .and_then(|t| t["sample_rate"].as_u64())
}

// ---------------------------------------------------------------------------
// Frame rate passthrough: H.265 → H.264 transcode
// ---------------------------------------------------------------------------

#[test]
#[ignore] // RED CELL: frame rate is not preserved through transcode (output shows fractional drift)
fn test_that_h265_transcode_preserves_frame_rate() {
    // GIVEN — an H.265 MP4 fixture with a known frame rate
    let input = fixture_path("bigbuckbunny_h265.mp4");
    let output_path = "/tmp/splica_test_rate_h265_transcode.mp4";
    let input_json = probe_json(&input);
    let input_fps = video_frame_rate(&input_json);
    assert!(
        input_fps.is_some(),
        "input fixture should report a frame rate"
    );

    // WHEN — transcode via resize (forces decode-encode path)
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &input,
            "-o",
            output_path,
            "--resize",
            "320x180",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "H.265 transcode should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — output frame rate matches input frame rate exactly
    let output_json = probe_json(output_path);
    let output_fps = video_frame_rate(&output_json);

    assert_eq!(
        input_fps, output_fps,
        "frame rate should be preserved through transcode. input: {input_fps:?}, output: {output_fps:?}"
    );

    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// Frame rate passthrough: AV1 → H.264 transcode
// ---------------------------------------------------------------------------

#[test]
#[ignore] // RED CELL: frame rate is not preserved through transcode (output shows fractional drift)
fn test_that_av1_transcode_preserves_frame_rate() {
    // GIVEN — an AV1 MP4 fixture with a known frame rate
    let input = fixture_path("bigbuckbunny_av1.mp4");
    let output_path = "/tmp/splica_test_rate_av1_transcode.mp4";
    let input_json = probe_json(&input);
    let input_fps = video_frame_rate(&input_json);
    assert!(
        input_fps.is_some(),
        "input fixture should report a frame rate"
    );

    // WHEN — transcode via resize (forces decode-encode path)
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &input,
            "-o",
            output_path,
            "--resize",
            "320x180",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "AV1 transcode should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — output frame rate matches input frame rate exactly
    let output_json = probe_json(output_path);
    let output_fps = video_frame_rate(&output_json);

    assert_eq!(
        input_fps, output_fps,
        "frame rate should be preserved through transcode. input: {input_fps:?}, output: {output_fps:?}"
    );

    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// Sample rate passthrough (audio)
// ---------------------------------------------------------------------------
//
// NOTE: None of the current fixtures contain audio tracks, so we cannot test
// audio sample rate passthrough yet. When fixtures with audio are added, a test
// should be added here that transcodes a file with audio and verifies the
// sample rate (e.g. 48000 Hz) is preserved in the output.
//
// Skeleton for future use:
//
// #[test]
// fn test_that_transcode_preserves_audio_sample_rate() {
//     let input = fixture_path("some_fixture_with_audio.mp4");
//     let output_path = "/tmp/splica_test_rate_audio.mp4";
//     let input_json = probe_json(&input);
//     let input_sr = audio_sample_rate(&input_json)
//         .expect("input fixture should have an audio track with sample_rate");
//
//     let output = splica_binary()
//         .args(["process", "-i", &input, "-o", output_path, "--resize", "320x180"])
//         .output()
//         .unwrap();
//     assert!(output.status.success());
//
//     let output_json = probe_json(output_path);
//     let output_sr = audio_sample_rate(&output_json)
//         .expect("output should have an audio track with sample_rate");
//     assert_eq!(input_sr, output_sr,
//         "audio sample rate should be preserved through transcode");
//
//     let _ = std::fs::remove_file(output_path);
// }
