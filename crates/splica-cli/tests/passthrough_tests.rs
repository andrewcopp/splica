//! Frame rate and sample rate passthrough tests.
//!
//! Verifies that frame rate and audio sample rate survive stream copy
//! operations through the pipeline.

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

// ---------------------------------------------------------------------------
// Frame rate passthrough — stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_stream_copy_preserves_frame_rate_h265_mp4() {
    let output_path = "/tmp/splica_test_fps_h265.mp4";

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265.mp4"),
            "-o",
            output_path,
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stream copy should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = probe_json(output_path);
    let _ = std::fs::remove_file(output_path);

    let frame_rate = json["tracks"][0]["frame_rate"].as_str();
    assert_eq!(
        frame_rate,
        Some("30"),
        "frame rate should be preserved through stream copy"
    );
}

#[test]
fn test_that_stream_copy_preserves_frame_rate_h264_mp4() {
    let output_path = "/tmp/splica_test_fps_h264.mp4";

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mp4"),
            "-o",
            output_path,
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stream copy should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = probe_json(output_path);
    let _ = std::fs::remove_file(output_path);

    let frame_rate = json["tracks"][0]["frame_rate"].as_str();
    assert_eq!(
        frame_rate,
        Some("30"),
        "frame rate should be preserved through stream copy"
    );
}

// ---------------------------------------------------------------------------
// Sample rate passthrough — stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_stream_copy_preserves_audio_sample_rate_aac_mp4() {
    let output_path = "/tmp/splica_test_sr_aac.mp4";

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265_aac.mp4"),
            "-o",
            output_path,
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stream copy should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = probe_json(output_path);
    let _ = std::fs::remove_file(output_path);

    let sample_rate = json["tracks"][1]["sample_rate"].as_u64();
    assert_eq!(
        sample_rate,
        Some(44100),
        "audio sample rate (44100) should be preserved through stream copy"
    );
}

#[test]
fn test_that_stream_copy_preserves_audio_sample_rate_opus_webm() {
    let output_path = "/tmp/splica_test_sr_opus.webm";

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_vp9_opus.webm"),
            "-o",
            output_path,
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stream copy should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = probe_json(output_path);
    let _ = std::fs::remove_file(output_path);

    // Find the audio track
    let audio_track = json["tracks"]
        .as_array()
        .and_then(|tracks| tracks.iter().find(|t| t["kind"] == "audio"));

    assert!(audio_track.is_some(), "output should have an audio track");
    let sample_rate = audio_track.unwrap()["sample_rate"].as_u64();
    assert_eq!(
        sample_rate,
        Some(48000),
        "audio sample rate (48000) should be preserved through stream copy"
    );
}

// ---------------------------------------------------------------------------
// Channel layout passthrough
// ---------------------------------------------------------------------------

#[test]
fn test_that_stream_copy_preserves_channel_layout_mp4() {
    let output_path = "/tmp/splica_test_channels.mp4";

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265_aac.mp4"),
            "-o",
            output_path,
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stream copy should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = probe_json(output_path);
    let _ = std::fs::remove_file(output_path);

    let channels = json["tracks"][1]["channels"].as_u64();
    assert!(
        channels.is_some(),
        "channel count should be present in output"
    );

    let channel_layout = json["tracks"][1]["channel_layout"].as_str();
    assert!(
        channel_layout.is_some(),
        "channel layout should be present in output"
    );
}
