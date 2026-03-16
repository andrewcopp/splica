//! Color space passthrough tests.
//!
//! Verifies that color space metadata (BT.709, BT.2020, etc.) survives
//! stream copy and transcode operations through the pipeline.

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

fn probe_color_space(path: &str) -> Option<String> {
    let output = splica_binary()
        .args(["probe", "--format", "json", path])
        .output()
        .unwrap();

    let json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();

    json["tracks"][0]["color_space"]
        .as_str()
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// Color space detection
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_detects_bt709_color_space_in_h265_mp4() {
    let color = probe_color_space(&fixture_path("bigbuckbunny_h265_bt709.mp4"));

    assert_eq!(
        color.as_deref(),
        Some("bt709/limited"),
        "expected bt709/limited color space in H.265 fixture"
    );
}

#[test]
fn test_that_probe_reports_null_color_space_when_absent() {
    let color = probe_color_space(&fixture_path("bigbuckbunny_h264.mp4"));

    assert_eq!(
        color, None,
        "expected no color space in H.264 fixture without colr box"
    );
}

// ---------------------------------------------------------------------------
// Stream copy preserves color space
// ---------------------------------------------------------------------------

#[test]
fn test_that_stream_copy_preserves_bt709_color_space() {
    let output_path = "/tmp/splica_test_color_stream_copy.mp4";

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265_bt709.mp4"),
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

    let color = probe_color_space(output_path);
    let _ = std::fs::remove_file(output_path);

    assert_eq!(
        color.as_deref(),
        Some("bt709/limited"),
        "color space should survive stream copy"
    );
}

// ---------------------------------------------------------------------------
// Re-encode preserves color space
// ---------------------------------------------------------------------------

#[test]
fn test_that_reencode_h265_to_h264_preserves_bt709_color_space() {
    let output_path = "/tmp/splica_test_color_reencode_h265_h264.mp4";

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265_bt709.mp4"),
            "-o",
            output_path,
            "--resize",
            "320x180",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "re-encode should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let color = probe_color_space(output_path);
    let _ = std::fs::remove_file(output_path);

    assert_eq!(
        color.as_deref(),
        Some("bt709/limited"),
        "color space should survive H.265→H.264 re-encode"
    );
}

#[test]
fn test_that_reencode_h265_to_h265_preserves_bt709_color_space() {
    let output_path = "/tmp/splica_test_color_reencode_h265_h265.mp4";

    // Use 320x192 (multiple of 64) to avoid kvazaar CTU alignment assertion.
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265_bt709.mp4"),
            "-o",
            output_path,
            "--resize",
            "320x192",
            "--codec",
            "h265",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "re-encode should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let color = probe_color_space(output_path);
    let _ = std::fs::remove_file(output_path);

    assert_eq!(
        color.as_deref(),
        Some("bt709/limited"),
        "color space should survive H.265→H.265 re-encode"
    );
}

#[test]
fn test_that_reencode_preserves_unspecified_color_space() {
    let output_path = "/tmp/splica_test_color_reencode_unspecified.mp4";

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265.mp4"),
            "-o",
            output_path,
            "--resize",
            "320x180",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "re-encode should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let color = probe_color_space(output_path);
    let _ = std::fs::remove_file(output_path);

    assert_eq!(
        color, None,
        "unspecified color space should remain absent after re-encode"
    );
}
