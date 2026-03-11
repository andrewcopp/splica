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
// MP4 AV1 probe
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_reports_av1_codec_for_real_mp4() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_av1.mp4")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AV1"), "expected AV1 in output: {stdout}");
}

#[test]
fn test_that_probe_reports_correct_av1_resolution() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_av1.mp4")])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("640x360"),
        "expected 640x360 in output: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Color space
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_json_reports_null_color_space_for_bbb_h264() {
    // BBB H.264 fixture has no colr box and no SPS VUI color description,
    // so color_space should be null.
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
    let video_track = &json["tracks"][0];
    assert!(
        video_track["color_space"].is_null(),
        "expected null color_space for BBB H.264 (no colr box, no VUI color), got: {}",
        video_track["color_space"]
    );
}

// ---------------------------------------------------------------------------
// Process JSON output contract
// ---------------------------------------------------------------------------

#[test]
fn test_that_process_json_includes_audio_tracks_array() {
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mp4"),
            "-o",
            "/tmp/splica_test_json_contract.mp4",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["type"], "complete");
    assert!(json["audio_tracks"].is_array());

    // Clean up
    let _ = std::fs::remove_file("/tmp/splica_test_json_contract.mp4");
}

#[test]
fn test_that_process_json_complete_event_includes_qc_fields() {
    // GIVEN — a known fixture
    let output_path = "/tmp/splica_test_json_qc.mp4";

    // WHEN — process with --format json
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mp4"),
            "-o",
            output_path,
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "process should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // THEN — complete event has QC fields
    assert_eq!(json["type"], "complete");
    assert_eq!(json["mux_ok"], true);
    assert!(
        json["output_codec"].is_string(),
        "output_codec should be a string"
    );
    assert!(
        json["output_duration_secs"].is_number(),
        "output_duration_secs should be a number"
    );
    assert!(
        json["output_bitrate_kbps"].is_number(),
        "output_bitrate_kbps should be a number"
    );

    // Duration should be plausible (BBB clip is ~2s)
    let duration = json["output_duration_secs"].as_f64().unwrap();
    assert!(duration > 0.5, "duration too short: {duration}");
    assert!(duration < 30.0, "duration too long: {duration}");

    // Bitrate should be plausible (> 0 kbps)
    let bitrate = json["output_bitrate_kbps"].as_u64().unwrap();
    assert!(bitrate > 0, "bitrate should be positive");

    // Clean up
    let _ = std::fs::remove_file(output_path);
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

// ---------------------------------------------------------------------------
// Error kind distinction (SPL-86)
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_json_of_corrupt_file_reports_bad_input_error_kind() {
    // A corrupt file (invalid magic bytes) should produce error_kind "bad_input",
    // not "unsupported_format".
    let dir = std::env::temp_dir().join("splica_error_kind_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("corrupt_error_kind.mp4");
    std::fs::write(&path, b"this is definitely not a valid media file").unwrap();

    let output = splica_binary()
        .args(["probe", "--format", "json", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(
        json["error_kind"], "bad_input",
        "corrupt file should produce 'bad_input', got: {}",
        json["error_kind"]
    );

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// H.265 decode (SPL-82)
// ---------------------------------------------------------------------------

#[test]
fn test_that_process_of_h265_with_resize_succeeds() {
    // H.265 input should be decodable via libde265 and re-encoded as H.264.
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265.mp4"),
            "-o",
            "/tmp/splica_test_h265_reencode.mp4",
            "--resize",
            "320x180",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "H.265 process with resize should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify output exists and is non-empty
    let meta = std::fs::metadata("/tmp/splica_test_h265_reencode.mp4").unwrap();
    assert!(meta.len() > 0, "output file should be non-empty");

    let _ = std::fs::remove_file("/tmp/splica_test_h265_reencode.mp4");
}
