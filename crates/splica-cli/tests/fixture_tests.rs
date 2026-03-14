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

// ---------------------------------------------------------------------------
// Crop filter (SPL-95)
// ---------------------------------------------------------------------------

#[test]
fn test_that_process_with_crop_succeeds() {
    // Crop a 320x180 region from the 640x360 H.265 fixture.
    // Uses H.265 input because it decodes reliably in this test harness.
    let output_path = "/tmp/splica_test_crop.mp4";
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265.mp4"),
            "-o",
            output_path,
            "--crop",
            "320x180+160+90",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "process with crop should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify output exists and is non-empty
    let meta = std::fs::metadata(output_path).unwrap();
    assert!(meta.len() > 0, "output file should be non-empty");

    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// AV1 decode (SPL-88)
// ---------------------------------------------------------------------------

#[test]
fn test_that_process_of_av1_with_resize_succeeds() {
    // AV1 input should be decodable via dav1d and re-encoded as H.264.
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_av1.mp4"),
            "-o",
            "/tmp/splica_test_av1_reencode.mp4",
            "--resize",
            "320x180",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "AV1 process with resize should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify output exists and is non-empty
    let meta = std::fs::metadata("/tmp/splica_test_av1_reencode.mp4").unwrap();
    assert!(meta.len() > 0, "output file should be non-empty");

    let _ = std::fs::remove_file("/tmp/splica_test_av1_reencode.mp4");
}

// ---------------------------------------------------------------------------
// AV1 encode (SPL-94)
// ---------------------------------------------------------------------------

#[test]
#[ignore] // rav1e is ~400s in debug mode; run with `cargo test -- --ignored`
fn test_that_process_to_webm_produces_av1_output() {
    // AV1 input → WebM output should re-encode as AV1 via rav1e.
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_av1.mp4"),
            "-o",
            "/tmp/splica_test_av1_encode.webm",
            "--resize",
            "320x180",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "AV1 to WebM (AV1 encode) should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify output exists and is non-empty
    let meta = std::fs::metadata("/tmp/splica_test_av1_encode.webm").unwrap();
    assert!(meta.len() > 0, "output file should be non-empty");

    // Probe the output to verify it reports AV1 codec
    let probe = splica_binary()
        .args([
            "probe",
            "--format",
            "json",
            "/tmp/splica_test_av1_encode.webm",
        ])
        .output()
        .unwrap();

    assert!(probe.status.success());
    let stdout = String::from_utf8_lossy(&probe.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let codec = json["tracks"][0]["codec"].as_str().unwrap();
    assert!(
        codec.contains("AV1"),
        "expected AV1 codec in output, got: {codec}"
    );

    let _ = std::fs::remove_file("/tmp/splica_test_av1_encode.webm");
}

// ---------------------------------------------------------------------------
// Extract audio JSON type discriminator (SPL-116)
// ---------------------------------------------------------------------------

#[test]
fn test_that_extract_audio_json_has_type_discriminator() {
    // GIVEN — fixture has no audio tracks, so extract-audio will produce a JSON error.
    // This verifies the error path includes the "type" discriminator.
    let output_path = "/tmp/splica_test_extract_audio_type.mp4";

    // WHEN — extract-audio with --format json on a video-only file
    let output = splica_binary()
        .args([
            "extract-audio",
            "-i",
            &fixture_path("bigbuckbunny_h264.mp4"),
            "-o",
            output_path,
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    // THEN — should fail with structured JSON error including "type"
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected valid JSON error, got error {e}: {stdout}"));

    assert_eq!(json["type"], "error");

    // Cleanup
    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// Per-track duration (SPL-119)
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_mp4_reports_track_durations() {
    // GIVEN — an MP4 fixture with known duration

    // WHEN — probe with --format json
    let output = splica_binary()
        .args([
            "probe",
            "--format",
            "json",
            &fixture_path("bigbuckbunny_h264.mp4"),
        ])
        .output()
        .unwrap();

    // THEN — every track has a non-null duration_seconds > 0
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let tracks = json["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty());
    for track in tracks {
        let dur = track["duration_seconds"].as_f64();
        assert!(
            dur.is_some(),
            "track {} duration_seconds should not be null",
            track["index"]
        );
        assert!(
            dur.unwrap() > 0.0,
            "track {} duration_seconds should be > 0, got {}",
            track["index"],
            dur.unwrap()
        );
    }
}

#[test]
fn test_that_probe_webm_reports_track_durations() {
    // GIVEN — a WebM fixture with known duration

    // WHEN — probe with --format json
    let output = splica_binary()
        .args([
            "probe",
            "--format",
            "json",
            &fixture_path("bigbuckbunny_vp9.webm"),
        ])
        .output()
        .unwrap();

    // THEN — every track has a non-null duration_seconds > 0
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let tracks = json["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty());
    for track in tracks {
        let dur = track["duration_seconds"].as_f64();
        assert!(
            dur.is_some(),
            "track {} duration_seconds should not be null",
            track["index"]
        );
        assert!(
            dur.unwrap() > 0.0,
            "track {} duration_seconds should be > 0, got {}",
            track["index"],
            dur.unwrap()
        );
    }
}

#[test]
fn test_that_probe_mkv_reports_track_durations() {
    // GIVEN — an MKV fixture with known duration

    // WHEN — probe with --format json
    let output = splica_binary()
        .args([
            "probe",
            "--format",
            "json",
            &fixture_path("bigbuckbunny_h264.mkv"),
        ])
        .output()
        .unwrap();

    // THEN — every track has a non-null duration_seconds > 0
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let tracks = json["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty());
    for track in tracks {
        let dur = track["duration_seconds"].as_f64();
        assert!(
            dur.is_some(),
            "track {} duration_seconds should not be null",
            track["index"]
        );
        assert!(
            dur.unwrap() > 0.0,
            "track {} duration_seconds should be > 0, got {}",
            track["index"],
            dur.unwrap()
        );
    }
}

// ---------------------------------------------------------------------------
// Migrate binary invocation (SPL-133)
// ---------------------------------------------------------------------------

#[test]
fn test_that_migrate_translates_ffmpeg_command_to_splica() {
    // GIVEN — a valid ffmpeg-style command

    // WHEN — invoking the migrate subcommand via the binary
    let output = splica_binary()
        .args([
            "migrate",
            "ffmpeg",
            "-i",
            "input.mp4",
            "-vf",
            "scale=1280:720",
            "output.webm",
        ])
        .output()
        .unwrap();

    // THEN — exits successfully and stdout contains the translated splica command
    assert!(
        output.status.success(),
        "migrate should exit successfully. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("splica process"),
        "expected 'splica process' in migrate output, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// AV1 encode to MP4 (SPL-133)
// ---------------------------------------------------------------------------

#[test]
#[ignore] // rav1e is very slow in debug mode; run with `cargo test -- --ignored`
fn test_that_process_with_codec_av1_to_mp4_produces_av1_output() {
    // GIVEN — an H.265 MP4 fixture (decodable via libde265)
    let output_path = "/tmp/splica_test_av1_in_mp4.mp4";

    // WHEN — process with --codec av1 targeting an MP4 output
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &fixture_path("bigbuckbunny_h265.mp4"),
            "-o",
            output_path,
            "--codec",
            "av1",
            "--resize",
            "320x180",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "AV1 encode to MP4 should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — probe the output to verify AV1 codec
    let probe = splica_binary()
        .args(["probe", "--format", "json", output_path])
        .output()
        .unwrap();

    assert!(probe.status.success());
    let stdout = String::from_utf8_lossy(&probe.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let codec = json["tracks"][0]["codec"].as_str().unwrap();
    assert!(
        codec.contains("AV1"),
        "expected AV1 codec in MP4 output, got: {codec}"
    );

    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// Probe JSON contract (SPL-116)
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_json_reports_container_and_duration() {
    // GIVEN — a known fixture

    // WHEN — probe with --format json
    let output = splica_binary()
        .args([
            "probe",
            "--format",
            "json",
            &fixture_path("bigbuckbunny_h264.mp4"),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "probe --format json should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — JSON has container, duration_seconds > 0, size_bytes > 0
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected valid JSON, got error {e}: {stdout}"));

    assert!(
        json["container"].as_str().is_some(),
        "expected container field, got: {}",
        json["container"]
    );
    assert!(
        json["duration_seconds"].as_f64().unwrap_or(0.0) > 0.0,
        "expected duration_seconds > 0, got: {}",
        json["duration_seconds"]
    );
    assert!(
        json["size_bytes"].as_u64().unwrap_or(0) > 0,
        "expected size_bytes > 0, got: {}",
        json["size_bytes"]
    );
}
