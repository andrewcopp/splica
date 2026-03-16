//! Tests that frame rate / PTS is preserved through transcode paths.
//!
//! When re-encoding through H.265 or AV1 encoders, the output duration must
//! closely match the input duration. A broken PTS pipeline (e.g., reconstructing
//! PTS from poc or frame number) would produce wildly wrong durations.

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

/// Probes a file and returns (duration_seconds, frame_rate_string) for the first track.
fn probe_duration(path: &str) -> f64 {
    let output = splica_binary()
        .args(["probe", "--format", "json", path])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "probe should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    json["tracks"][0]["duration_seconds"]
        .as_f64()
        .expect("expected duration_seconds in probe output")
}

// ---------------------------------------------------------------------------
// H.265 transcode PTS preservation
// ---------------------------------------------------------------------------

#[test]
fn test_that_h265_transcode_preserves_duration() {
    // GIVEN — the H.265 fixture with a known duration
    let input = fixture_path("bigbuckbunny_h265.mp4");
    let input_duration = probe_duration(&input);

    let output_path = "/tmp/splica_test_h265_rate_passthrough.mp4";

    // WHEN — transcode H.265 to H.265 (decode + re-encode, no resize)
    let output = splica_binary()
        .args([
            "process",
            "-i",
            &input,
            "-o",
            output_path,
            "--codec",
            "h265",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "H.265 transcode should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — output duration should be within 20% of input duration
    let output_duration = probe_duration(output_path);
    let ratio = output_duration / input_duration;
    assert!(
        (0.8..=1.2).contains(&ratio),
        "output duration {output_duration:.3}s should be within 20% of input {input_duration:.3}s (ratio: {ratio:.3})"
    );

    let _ = std::fs::remove_file(output_path);
}

// ---------------------------------------------------------------------------
// AV1 transcode PTS preservation
// ---------------------------------------------------------------------------

#[test]
#[ignore] // rav1e is very slow in debug mode; run with `cargo test -- --ignored`
fn test_that_av1_transcode_preserves_duration() {
    // GIVEN — the AV1 fixture with a known duration
    let input = fixture_path("bigbuckbunny_av1.mp4");
    let input_duration = probe_duration(&input);

    let output_path = "/tmp/splica_test_av1_rate_passthrough.mp4";

    // WHEN — transcode AV1 to AV1 (decode + re-encode, no resize)
    let output = splica_binary()
        .args(["process", "-i", &input, "-o", output_path, "--codec", "av1"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "AV1 transcode should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — output duration should be within 20% of input duration
    let output_duration = probe_duration(output_path);
    let ratio = output_duration / input_duration;
    assert!(
        (0.8..=1.2).contains(&ratio),
        "output duration {output_duration:.3}s should be within 20% of input {input_duration:.3}s (ratio: {ratio:.3})"
    );

    let _ = std::fs::remove_file(output_path);
}
