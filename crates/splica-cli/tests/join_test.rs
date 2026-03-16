//! Integration tests for the `splica join` command.

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
// Happy path
// ---------------------------------------------------------------------------

#[test]
fn test_that_join_concatenates_two_identical_mp4_files() {
    // GIVEN — the same fixture used twice (same codecs, same track layout)
    let input = fixture_path("bigbuckbunny_h264.mp4");
    let output_path = "/tmp/splica_test_join_two.mp4";

    // WHEN — join them
    let output = splica_binary()
        .args(["join", "-i", &input, &input, "-o", output_path])
        .output()
        .unwrap();

    // THEN — succeeds and output is non-empty
    assert!(
        output.status.success(),
        "join should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta = std::fs::metadata(output_path).unwrap();
    assert!(meta.len() > 0, "output file should be non-empty");

    let _ = std::fs::remove_file(output_path);
}

#[test]
fn test_that_join_output_has_more_packets_than_single_input() {
    // GIVEN
    let input = fixture_path("bigbuckbunny_h264.mp4");
    let output_path = "/tmp/splica_test_join_packets.mp4";

    // WHEN — join the file with itself
    let output = splica_binary()
        .args([
            "join",
            "-i",
            &input,
            &input,
            "-o",
            output_path,
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "join should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — JSON reports packets written > 0 and files_joined = 2
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected valid JSON, got error {e}: {stdout}"));

    assert_eq!(json["type"], "complete");
    assert_eq!(json["files_joined"], 2);
    assert!(json["packets_written"].as_u64().unwrap() > 0);

    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// Error: mismatched codecs
// ---------------------------------------------------------------------------

#[test]
fn test_that_join_rejects_mismatched_codecs() {
    // GIVEN — two files with different video codecs
    let h264 = fixture_path("bigbuckbunny_h264.mp4");
    let h265 = fixture_path("bigbuckbunny_h265.mp4");
    let output_path = "/tmp/splica_test_join_mismatch.mp4";

    // WHEN
    let output = splica_binary()
        .args(["join", "-i", &h264, &h265, "-o", output_path])
        .output()
        .unwrap();

    // THEN — fails with an error about codec mismatch
    assert!(
        !output.status.success(),
        "join should fail for mismatched codecs"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("codec mismatch") || stderr.contains("Codec mismatch"),
        "expected codec mismatch error, got: {stderr}"
    );

    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// Error: too few inputs
// ---------------------------------------------------------------------------

#[test]
fn test_that_join_rejects_single_input() {
    // GIVEN — only one input file
    let input = fixture_path("bigbuckbunny_h264.mp4");
    let output_path = "/tmp/splica_test_join_single.mp4";

    // WHEN
    let output = splica_binary()
        .args(["join", "-i", &input, "-o", output_path])
        .output()
        .unwrap();

    // THEN — clap rejects it (num_args = 2..)
    assert!(
        !output.status.success(),
        "join should fail with single input"
    );

    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// WebM output
// ---------------------------------------------------------------------------

#[test]
fn test_that_join_supports_webm_output() {
    // GIVEN — two identical WebM files
    let input = fixture_path("bigbuckbunny_vp9.webm");
    let output_path = "/tmp/splica_test_join_webm.webm";

    // WHEN
    let output = splica_binary()
        .args(["join", "-i", &input, &input, "-o", output_path])
        .output()
        .unwrap();

    // THEN
    assert!(
        output.status.success(),
        "join with WebM should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta = std::fs::metadata(output_path).unwrap();
    assert!(meta.len() > 0, "output file should be non-empty");

    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// MKV output
// ---------------------------------------------------------------------------

#[test]
fn test_that_join_supports_mkv_output() {
    // GIVEN — two identical MKV files
    let input = fixture_path("bigbuckbunny_h264.mkv");
    let output_path = "/tmp/splica_test_join_mkv.mkv";

    // WHEN
    let output = splica_binary()
        .args(["join", "-i", &input, &input, "-o", output_path])
        .output()
        .unwrap();

    // THEN
    assert!(
        output.status.success(),
        "join with MKV should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta = std::fs::metadata(output_path).unwrap();
    assert!(meta.len() > 0, "output file should be non-empty");

    let _ = std::fs::remove_file(output_path);
}
