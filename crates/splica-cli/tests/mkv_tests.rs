//! MKV end-to-end round-trip integration tests (SPL-99).
//!
//! Exercises the full pipeline path for MKV input/output, cross-container
//! transcodes, and error handling. The MKV fixture was generated once from
//! the existing MP4 H.264 fixture via:
//!
//!   ffmpeg -i tests/fixtures/bigbuckbunny_h264.mp4 -c copy tests/fixtures/bigbuckbunny_h264.mkv

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

fn test_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("splica_mkv_tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Asserts the process exited with code 1 (not 101 which indicates a panic).
fn assert_exit_code_1(output: &std::process::Output) {
    assert!(!output.status.success(), "expected failure but got success");
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert!(
            output.status.signal().is_none(),
            "process was killed by signal {:?}",
            output.status.signal()
        );
        assert_eq!(
            output.status.code(),
            Some(1),
            "expected exit code 1 (bad_input), got {:?} — a code of 101 indicates a panic",
            output.status.code()
        );
    }
}

// ---------------------------------------------------------------------------
// MKV probe
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_reports_h264_codec_for_mkv() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_h264.mkv")])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "probe should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("H.264"),
        "expected H.264 in output: {stdout}"
    );
}

#[test]
fn test_that_probe_reports_correct_mkv_resolution() {
    let output = splica_binary()
        .args(["probe", &fixture_path("bigbuckbunny_h264.mkv")])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("640x360"),
        "expected 640x360 in output: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// 1. MKV → MKV (stream copy)
// ---------------------------------------------------------------------------

#[test]
fn test_that_mkv_to_mkv_stream_copy_produces_valid_output() {
    // GIVEN — MKV fixture with H.264 video
    let output_path = test_dir().join("mkv_to_mkv.mkv");

    // WHEN — process MKV → MKV (H.264 is compatible with MKV, so stream copy)
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mkv"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // THEN — output is a valid MKV with video track
    assert!(
        output.status.success(),
        "MKV → MKV should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let meta = std::fs::metadata(&output_path).unwrap();
    assert!(meta.len() > 0, "output file should be non-empty");

    // Probe output to verify it's a valid MKV with H.264
    let probe = splica_binary()
        .args(["probe", "--format", "json", output_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(probe.status.success(), "probe of output should succeed");
    let stdout = String::from_utf8_lossy(&probe.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let tracks = json["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty(), "output should have at least one track");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// 2. MKV → MP4 (transcode)
// ---------------------------------------------------------------------------

#[test]
fn test_that_mkv_to_mp4_transcode_produces_valid_output() {
    // GIVEN — MKV fixture with H.264 video
    let output_path = test_dir().join("mkv_to_mp4.mp4");

    // WHEN — process MKV → MP4
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mkv"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // THEN — output is a valid MP4 that can be probed
    assert!(
        output.status.success(),
        "MKV → MP4 should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let probe = splica_binary()
        .args(["probe", "--format", "json", output_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(probe.status.success(), "probe of MP4 output should succeed");
    let stdout = String::from_utf8_lossy(&probe.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let tracks = json["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty(), "output should have at least one track");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// 3. MKV → WebM (transcode) — ignored, AV1 encode is slow in debug
// ---------------------------------------------------------------------------

#[test]
#[ignore] // rav1e is ~400s in debug mode; run with `cargo test -- --ignored`
fn test_that_mkv_to_webm_transcode_produces_valid_output() {
    // GIVEN — MKV fixture with H.264 video
    let output_path = test_dir().join("mkv_to_webm.webm");

    // WHEN — process MKV → WebM (requires AV1 re-encode via rav1e)
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mkv"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // THEN — output is a valid WebM
    assert!(
        output.status.success(),
        "MKV → WebM should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let probe = splica_binary()
        .args(["probe", "--format", "json", output_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        probe.status.success(),
        "probe of WebM output should succeed"
    );

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// 4. MP4 → MKV (transcode)
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_to_mkv_transcode_produces_valid_output() {
    // GIVEN — MP4 fixture with H.264 video
    let output_path = test_dir().join("mp4_to_mkv.mkv");

    // WHEN — process MP4 → MKV
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mp4"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // THEN — output is a valid MKV with correct codec
    assert!(
        output.status.success(),
        "MP4 → MKV should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let probe = splica_binary()
        .args(["probe", "--format", "json", output_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(probe.status.success(), "probe of MKV output should succeed");
    let stdout = String::from_utf8_lossy(&probe.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let tracks = json["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty(), "output should have at least one track");

    // Verify codec is H.264
    let codec = tracks[0]["codec"].as_str().unwrap();
    assert!(
        codec.contains("H.264"),
        "expected H.264 codec in MKV output, got: {codec}"
    );

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// 5. MKV color space passthrough
// ---------------------------------------------------------------------------

#[test]
fn test_that_mkv_color_space_is_preserved_through_round_trip() {
    // GIVEN — MKV fixture (BBB H.264 has no VUI color params → null color_space)
    // Verify the input has null color_space
    let input_probe = splica_binary()
        .args([
            "probe",
            "--format",
            "json",
            &fixture_path("bigbuckbunny_h264.mkv"),
        ])
        .output()
        .unwrap();
    assert!(input_probe.status.success());
    let input_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&input_probe.stdout)).unwrap();
    let input_color = &input_json["tracks"][0]["color_space"];

    // WHEN — round-trip through MKV → MKV
    let output_path = test_dir().join("mkv_color_roundtrip.mkv");
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mkv"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "color space round-trip should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — color_space in output matches input
    let output_probe = splica_binary()
        .args(["probe", "--format", "json", output_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output_probe.status.success());
    let output_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output_probe.stdout)).unwrap();
    let output_color = &output_json["tracks"][0]["color_space"];

    assert_eq!(
        input_color, output_color,
        "color_space should be preserved through MKV round-trip"
    );

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// CLI smoke: MKV → MP4 with JSON output
// ---------------------------------------------------------------------------

#[test]
fn test_that_mkv_to_mp4_cli_json_reports_complete() {
    let output_path = test_dir().join("mkv_to_mp4_json.mp4");

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mkv"),
            "-o",
            output_path.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "MKV → MP4 JSON should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["type"], "complete");
    assert_eq!(json["mux_ok"], true);

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// CLI smoke: MP4 → MKV with JSON output
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_to_mkv_cli_json_reports_complete() {
    let output_path = test_dir().join("mp4_to_mkv_json.mkv");

    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h264.mp4"),
            "-o",
            output_path.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "MP4 → MKV JSON should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["type"], "complete");
    assert_eq!(json["mux_ok"], true);

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// Error case: malformed MKV
// ---------------------------------------------------------------------------

fn build_truncated_mkv() -> Vec<u8> {
    let mut data = Vec::new();
    // EBML header magic (identifies as WebM/MKV)
    data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]);
    // Minimal EBML header element (truncated — just the ID, no valid body)
    data.extend_from_slice(&[0x85]); // size = 5
    data.extend_from_slice(&[0x00; 5]); // garbage body
    data
}

#[test]
fn test_that_probe_of_malformed_mkv_exits_with_error_not_panic() {
    let path = test_dir().join("malformed.mkv");
    std::fs::write(&path, build_truncated_mkv()).unwrap();

    let output = splica_binary()
        .args(["probe", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_json_of_malformed_mkv_reports_bad_input() {
    let path = test_dir().join("malformed_json.mkv");
    std::fs::write(&path, build_truncated_mkv()).unwrap();

    let output = splica_binary()
        .args(["probe", "--format", "json", path.to_str().unwrap()])
        .output()
        .unwrap();

    assert_exit_code_1(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("error output should be valid JSON");
    assert_eq!(json["type"], "error");
    assert_eq!(json["error_kind"], "bad_input");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_process_of_malformed_mkv_exits_with_error_not_panic() {
    let dir = test_dir();
    let input = dir.join("malformed_process.mkv");
    let output_path = dir.join("malformed_process_out.mkv");
    std::fs::write(&input, build_truncated_mkv()).unwrap();

    let output = splica_binary()
        .args([
            "process",
            "-i",
            input.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}
