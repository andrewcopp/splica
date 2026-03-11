//! Integration tests using real media fixture files.
//!
//! These tests verify that the splica CLI and demuxers correctly parse
//! real-world media files. Each fixture is a CC-licensed Big Buck Bunny
//! clip from test-videos.co.uk.

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

// ---------------------------------------------------------------------------
// MP4 H.264 probe
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_reports_h264_codec_for_real_mp4() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_h264.mp4")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("H.264"),
        "expected H.264 in output: {stdout}"
    );
}

#[test]
fn test_that_probe_reports_correct_h264_resolution() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_h264.mp4")])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("640x360"),
        "expected 640x360 in output: {stdout}"
    );
}

#[test]
fn test_that_probe_json_reports_h264_track_count() {
    let output = splica_binary()
        .args([
            "probe",
            "--format",
            "json",
            &fixture_path("bigbuckbunny_h264.mp4"),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let tracks = json["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
}

// ---------------------------------------------------------------------------
// MP4 H.265 probe
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_reports_h265_codec_for_real_mp4() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_h265.mp4")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("H.265"),
        "expected H.265 in output: {stdout}"
    );
}

#[test]
fn test_that_probe_reports_correct_h265_resolution() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_h265.mp4")])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("640x360"),
        "expected 640x360 in output: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// WebM VP9 probe
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_reports_vp9_codec_for_real_webm() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_vp9.webm")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("VP9"), "expected VP9 in output: {stdout}");
}

#[test]
fn test_that_probe_reports_correct_vp9_resolution() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_vp9.webm")])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("640x360"),
        "expected 640x360 in output: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[test]
fn test_that_process_of_vp9_with_resize_produces_clear_error() {
    // VP9 can't be re-encoded with our H.264-only encoder.
    // --resize forces re-encoding, which triggers the error.
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_vp9.webm"),
            "-o",
            "/tmp/splica_test_vp9_out.mp4",
            "--resize",
            "640x480",
        ])
        .output()
        .unwrap();

    // Should fail with a clear error, not silently produce wrong output
    assert!(
        !output.status.success(),
        "expected process of VP9 with resize to fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("error") || combined.contains("Error") || combined.contains("H.264"),
        "expected error message about H.264, got: {combined}"
    );
}
