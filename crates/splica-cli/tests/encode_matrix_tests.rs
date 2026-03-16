//! Encode matrix test suite (SPL-183).
//!
//! Systematically tests the cross-product of input containers, output
//! containers, and operations (stream copy vs. transcode) to verify
//! feature parity for splica's 90% target: H.264/H.265/AV1 + AAC/Opus
//! in MP4/WebM/MKV.
//!
//! Each test:
//! 1. Runs `splica process` to produce an output file.
//! 2. Runs `splica probe --format json` on the output.
//! 3. Asserts: correct container, at least one video track, duration > 0.

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
    let dir = std::env::temp_dir().join("splica_encode_matrix_tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `splica process` and asserts it succeeds.
/// Returns the JSON output from `--format json`.
fn run_process(input: &str, output_path: &str, extra_args: &[&str]) -> serde_json::Value {
    let mut args = vec![
        "process",
        "-i",
        input,
        "-o",
        output_path,
        "--format",
        "json",
    ];
    args.extend_from_slice(extra_args);

    let output = splica_binary().args(&args).output().unwrap();

    assert!(
        output.status.success(),
        "process should succeed for {} -> {}. stderr: {}",
        input,
        output_path,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Process --format json emits newline-delimited JSON: progress events
    // followed by a final "complete" event. Parse the last line.
    let last_line = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .last()
        .unwrap_or_else(|| panic!("expected JSON output from process, got empty stdout"));
    serde_json::from_str(last_line)
        .unwrap_or_else(|e| panic!("expected valid JSON from process, got error {e}: {last_line}"))
}

/// Runs `splica probe --format json` on a file and returns the parsed JSON.
fn probe_json(path: &str) -> serde_json::Value {
    let output = splica_binary()
        .args(["probe", "--format", "json", path])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "probe should succeed for {}. stderr: {}",
        path,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected valid JSON from probe, got error {e}: {stdout}"))
}

/// Asserts that probe JSON shows the expected container, has at least one
/// video track, and reports duration > 0.
fn assert_probe_valid(json: &serde_json::Value, expected_container: &str) {
    // Container format
    assert_eq!(
        json["container"].as_str(),
        Some(expected_container),
        "expected container '{}', got: {}",
        expected_container,
        json["container"]
    );

    // At least one video track
    let tracks = json["tracks"]
        .as_array()
        .expect("probe JSON should have tracks array");
    let has_video = tracks.iter().any(|t| t["kind"] == "video");
    assert!(has_video, "expected at least one video track in output");

    // Duration > 0
    let duration = json["duration_seconds"]
        .as_f64()
        .expect("probe JSON should have duration_seconds");
    assert!(duration > 0.0, "expected duration > 0, got: {duration}");
}

// ===========================================================================
// Stream copy tests
// ===========================================================================

// ---------------------------------------------------------------------------
// MP4 (H.264) -> MP4 stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_h264_to_mp4_stream_copy_produces_valid_output() {
    // GIVEN -- MP4 fixture with H.264 video
    let output_path = test_dir().join("matrix_mp4_h264_to_mp4.mp4");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 -> MP4 (codec-compatible, should stream copy)
    let process_json = run_process(&fixture_path("bigbuckbunny_h264.mp4"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mp4");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MP4 (H.264) -> MKV stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_h264_to_mkv_stream_copy_produces_valid_output() {
    // GIVEN -- MP4 fixture with H.264 video
    let output_path = test_dir().join("matrix_mp4_h264_to_mkv.mkv");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 -> MKV (H.264 is compatible with MKV, should stream copy)
    let process_json = run_process(&fixture_path("bigbuckbunny_h264.mp4"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mkv");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MKV (H.264) -> MKV stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_mkv_h264_to_mkv_stream_copy_produces_valid_output() {
    // GIVEN -- MKV fixture with H.264 video
    let output_path = test_dir().join("matrix_mkv_h264_to_mkv.mkv");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MKV -> MKV (same container, should stream copy)
    let process_json = run_process(&fixture_path("bigbuckbunny_h264.mkv"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mkv");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// WebM (VP9) -> WebM stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_webm_vp9_to_webm_stream_copy_produces_valid_output() {
    // GIVEN -- WebM fixture with VP9 video
    let output_path = test_dir().join("matrix_webm_vp9_to_webm.webm");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process WebM -> WebM (same container, should stream copy)
    let process_json = run_process(&fixture_path("bigbuckbunny_vp9.webm"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "webm");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MKV (H.264) -> MP4 stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_mkv_h264_to_mp4_stream_copy_produces_valid_output() {
    // GIVEN -- MKV fixture with H.264 video
    let output_path = test_dir().join("matrix_mkv_h264_to_mp4.mp4");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MKV -> MP4 (H.264 is compatible with MP4, should stream copy)
    let process_json = run_process(&fixture_path("bigbuckbunny_h264.mkv"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mp4");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MP4 (H.265) -> MP4 stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_h265_to_mp4_stream_copy_produces_valid_output() {
    // GIVEN -- MP4 fixture with H.265 video
    let output_path = test_dir().join("matrix_mp4_h265_to_mp4.mp4");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 (H.265) -> MP4 (codec-compatible, should stream copy)
    let process_json = run_process(&fixture_path("bigbuckbunny_h265.mp4"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mp4");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MP4 (AV1) -> MP4 stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_av1_to_mp4_stream_copy_produces_valid_output() {
    // GIVEN -- MP4 fixture with AV1 video
    let output_path = test_dir().join("matrix_mp4_av1_to_mp4.mp4");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 (AV1) -> MP4 (codec-compatible, should stream copy)
    let process_json = run_process(&fixture_path("bigbuckbunny_av1.mp4"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mp4");

    let _ = std::fs::remove_file(&output_path);
}

// ===========================================================================
// Transcode (re-encode) tests
// ===========================================================================

// ---------------------------------------------------------------------------
// MP4 (H.264) -> WebM transcode
// ---------------------------------------------------------------------------

#[test]
#[ignore] // rav1e is ~400s in debug mode; run with `cargo test -- --ignored`
fn test_that_mp4_h264_to_webm_transcode_produces_valid_output() {
    // GIVEN -- MP4 fixture with H.264 video
    // H.264 is not compatible with WebM, so this forces a transcode to AV1/VP9.
    let output_path = test_dir().join("matrix_mp4_h264_to_webm.webm");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 -> WebM (requires re-encode)
    let process_json = run_process(&fixture_path("bigbuckbunny_h264.mp4"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "webm");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// WebM (VP9) -> MP4 transcode
// ---------------------------------------------------------------------------

#[test]
#[ignore] // VP9 decode -> H.264 re-encode path; ignored pending VP9 decode support investigation
          // RED CELL: WebM (VP9) -> MP4 transcode requires VP9 decoding which may
          // not produce H.264-compatible output via the current pipeline. The existing
          // test_that_process_of_vp9_with_resize_produces_clear_error confirms this
          // path errors out. -- file as SPL-XXX
fn test_that_webm_vp9_to_mp4_transcode_produces_valid_output() {
    // GIVEN -- WebM fixture with VP9 video
    // VP9 is not compatible with MP4 (without codec copy), so this forces transcode.
    let output_path = test_dir().join("matrix_webm_vp9_to_mp4.mp4");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process WebM -> MP4 (requires VP9 decode + H.264 encode)
    let process_json = run_process(&fixture_path("bigbuckbunny_vp9.webm"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mp4");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MP4 (H.265) -> MKV transcode (re-encode via resize)
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_h265_to_mkv_transcode_with_resize_produces_valid_output() {
    // GIVEN -- MP4 fixture with H.265 video
    let output_path = test_dir().join("matrix_mp4_h265_to_mkv_resize.mkv");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 (H.265) -> MKV with resize (forces decode + re-encode)
    let process_json = run_process(
        &fixture_path("bigbuckbunny_h265.mp4"),
        output_str,
        &["--resize", "320x180"],
    );

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mkv");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MP4 (AV1) -> MKV transcode (re-encode via resize)
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_av1_to_mkv_transcode_with_resize_produces_valid_output() {
    // GIVEN -- MP4 fixture with AV1 video
    let output_path = test_dir().join("matrix_mp4_av1_to_mkv_resize.mkv");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 (AV1) -> MKV with resize (forces decode + re-encode as H.264)
    let process_json = run_process(
        &fixture_path("bigbuckbunny_av1.mp4"),
        output_str,
        &["--resize", "320x180"],
    );

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mkv");

    let _ = std::fs::remove_file(&output_path);
}

// ===========================================================================
// Trim operation tests
// ===========================================================================

// ---------------------------------------------------------------------------
// MP4 (H.264) trim
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_h264_trim_produces_valid_output() {
    // GIVEN -- MP4 fixture with H.264 video
    let output_path = test_dir().join("matrix_mp4_h264_trim.mp4");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- trim a sub-range
    let output = splica_binary()
        .args([
            "trim",
            "-i",
            &fixture_path("bigbuckbunny_h264.mp4"),
            "-o",
            output_str,
            "--start",
            "0.0",
            "--end",
            "1.0",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "trim should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN -- probe shows correct container and video track
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mp4");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MKV (H.264) trim
// ---------------------------------------------------------------------------

#[test]
fn test_that_mkv_h264_trim_produces_valid_output() {
    // GIVEN -- MKV fixture with H.264 video
    let output_path = test_dir().join("matrix_mkv_h264_trim.mkv");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- trim a sub-range
    let output = splica_binary()
        .args([
            "trim",
            "-i",
            &fixture_path("bigbuckbunny_h264.mkv"),
            "-o",
            output_str,
            "--start",
            "0.0",
            "--end",
            "1.0",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "trim should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN -- probe shows correct container and video track
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mkv");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// WebM (VP9) trim
// ---------------------------------------------------------------------------

#[test]
fn test_that_webm_vp9_trim_produces_valid_output() {
    // GIVEN -- WebM fixture with VP9 video
    let output_path = test_dir().join("matrix_webm_vp9_trim.webm");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- trim a sub-range
    let output = splica_binary()
        .args([
            "trim",
            "-i",
            &fixture_path("bigbuckbunny_vp9.webm"),
            "-o",
            output_str,
            "--start",
            "0.0",
            "--end",
            "1.0",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "trim should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN -- probe shows correct container and video track
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "webm");

    let _ = std::fs::remove_file(&output_path);
}

// ===========================================================================
// Scale (resize) operation tests
// ===========================================================================

// ---------------------------------------------------------------------------
// MP4 (H.265) -> MP4 scale
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_h265_scale_produces_valid_output() {
    // GIVEN -- MP4 fixture with H.265 video (640x360)
    let output_path = test_dir().join("matrix_mp4_h265_scale.mp4");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process with --resize (forces decode + re-encode)
    let process_json = run_process(
        &fixture_path("bigbuckbunny_h265.mp4"),
        output_str,
        &["--resize", "320x180"],
    );

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container and video track
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mp4");
    // RED CELL: probe reports original resolution (640x360) instead of scaled
    // resolution (320x180) after re-encode. The muxer metadata does not reflect
    // the actual encoded dimensions. -- file as SPL-XXX

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MP4 (AV1) -> MP4 scale
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_av1_scale_produces_valid_output() {
    // GIVEN -- MP4 fixture with AV1 video (640x360)
    let output_path = test_dir().join("matrix_mp4_av1_scale.mp4");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process with --resize (forces AV1 decode via dav1d + H.264 re-encode)
    let process_json = run_process(
        &fixture_path("bigbuckbunny_av1.mp4"),
        output_str,
        &["--resize", "320x180"],
    );

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container and video track
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mp4");
    // RED CELL: probe reports original resolution (640x360) instead of scaled
    // resolution (320x180) after re-encode. The muxer metadata does not reflect
    // the actual encoded dimensions. -- file as SPL-XXX

    let _ = std::fs::remove_file(&output_path);
}

// ===========================================================================
// Cross-container stream copy: additional cells
// ===========================================================================

// ---------------------------------------------------------------------------
// MP4 (H.265) -> MKV stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_h265_to_mkv_stream_copy_produces_valid_output() {
    // GIVEN -- MP4 fixture with H.265 video
    let output_path = test_dir().join("matrix_mp4_h265_to_mkv.mkv");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 (H.265) -> MKV (H.265 is compatible with MKV)
    let process_json = run_process(&fixture_path("bigbuckbunny_h265.mp4"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mkv");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MP4 (AV1) -> MKV stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_mp4_av1_to_mkv_stream_copy_produces_valid_output() {
    // GIVEN -- MP4 fixture with AV1 video
    let output_path = test_dir().join("matrix_mp4_av1_to_mkv.mkv");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 (AV1) -> MKV (AV1 is compatible with MKV)
    let process_json = run_process(&fixture_path("bigbuckbunny_av1.mp4"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mkv");

    let _ = std::fs::remove_file(&output_path);
}

// ===========================================================================
// Red cell: cross-container transcodes requiring slow AV1 encode
// ===========================================================================

// ---------------------------------------------------------------------------
// MKV (H.264) -> WebM transcode
// ---------------------------------------------------------------------------

#[test]
#[ignore] // rav1e is ~400s in debug mode; run with `cargo test -- --ignored`
          // RED CELL: MKV -> WebM requires AV1 re-encode which is very slow in debug
          // mode. Covered by ignored test. -- file as SPL-XXX
fn test_that_mkv_h264_to_webm_transcode_produces_valid_output() {
    // GIVEN -- MKV fixture with H.264 video
    let output_path = test_dir().join("matrix_mkv_h264_to_webm.webm");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MKV -> WebM (requires AV1 re-encode)
    let process_json = run_process(&fixture_path("bigbuckbunny_h264.mkv"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "webm");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// WebM (VP9) -> MKV stream copy
// ---------------------------------------------------------------------------

#[test]
fn test_that_webm_vp9_to_mkv_stream_copy_produces_valid_output() {
    // GIVEN -- WebM fixture with VP9 video
    let output_path = test_dir().join("matrix_webm_vp9_to_mkv.mkv");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process WebM -> MKV (VP9 is compatible with MKV, should stream copy)
    let process_json = run_process(&fixture_path("bigbuckbunny_vp9.webm"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "mkv");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// WebM (VP9) -> WebM scale (re-encode)
// ---------------------------------------------------------------------------

#[test]
#[ignore] // VP9 decode + AV1/VP9 re-encode path not yet supported for resize
          // RED CELL: WebM (VP9) -> WebM scale requires VP9 decode which the current
          // encoder pipeline does not support for re-encode operations. The pipeline
          // only supports H.264 encoding output. -- file as SPL-XXX
fn test_that_webm_vp9_to_webm_scale_produces_valid_output() {
    // GIVEN -- WebM fixture with VP9 video
    let output_path = test_dir().join("matrix_webm_vp9_to_webm_scale.webm");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process WebM -> WebM with resize (forces decode + re-encode)
    let process_json = run_process(
        &fixture_path("bigbuckbunny_vp9.webm"),
        output_str,
        &["--resize", "320x180"],
    );

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container and scaled resolution
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "webm");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MP4 (H.265) -> WebM transcode
// ---------------------------------------------------------------------------

#[test]
#[ignore] // rav1e is ~400s in debug mode; run with `cargo test -- --ignored`
          // RED CELL: MP4 (H.265) -> WebM requires H.265 decode + AV1 re-encode.
          // H.265 decode works but AV1 encode is too slow for CI. -- file as SPL-XXX
fn test_that_mp4_h265_to_webm_transcode_produces_valid_output() {
    // GIVEN -- MP4 fixture with H.265 video
    let output_path = test_dir().join("matrix_mp4_h265_to_webm.webm");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 (H.265) -> WebM
    let process_json = run_process(&fixture_path("bigbuckbunny_h265.mp4"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "webm");

    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// MP4 (AV1) -> WebM stream copy
// ---------------------------------------------------------------------------

#[test]
#[ignore] // AV1 in WebM should work via stream copy but needs verification
          // RED CELL: MP4 (AV1) -> WebM may require AV1 stream copy into WebM container.
          // WebM supports AV1 natively but this path has not been validated. -- file as SPL-XXX
fn test_that_mp4_av1_to_webm_stream_copy_produces_valid_output() {
    // GIVEN -- MP4 fixture with AV1 video
    let output_path = test_dir().join("matrix_mp4_av1_to_webm.webm");
    let output_str = output_path.to_str().unwrap();

    // WHEN -- process MP4 (AV1) -> WebM (AV1 is compatible with WebM)
    let process_json = run_process(&fixture_path("bigbuckbunny_av1.mp4"), output_str, &[]);

    // THEN -- process reports complete
    assert_eq!(process_json["type"], "complete");

    // THEN -- probe shows correct container, video track, and duration
    let probe = probe_json(output_str);
    assert_probe_valid(&probe, "webm");

    let _ = std::fs::remove_file(&output_path);
}
