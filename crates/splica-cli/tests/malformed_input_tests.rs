//! Integration tests for malformed input handling (SPL-58).
//!
//! Verifies that splica produces structured errors (not panics or hangs)
//! when given zero-byte files, truncated files, corrupt headers, and
//! extension-mismatched files.

use std::process::Command;
fn splica_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_splica"))
}

fn test_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("splica_malformed_tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs a command, guarding against hangs. The demuxers should fail fast
/// on invalid input — if this blocks, something is wrong.
fn run_with_timeout(cmd: &mut Command) -> std::process::Output {
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn process")
        .wait_with_output()
        .expect("failed to wait on process")
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
// Zero-byte file
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_of_zero_byte_file_exits_with_error() {
    let path = test_dir().join("zero.mp4");
    std::fs::write(&path, b"").unwrap();

    let output = run_with_timeout(splica_binary().args(["probe", path.to_str().unwrap()]));

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_json_of_zero_byte_file_reports_bad_input() {
    let path = test_dir().join("zero_json.mp4");
    std::fs::write(&path, b"").unwrap();

    let output = run_with_timeout(splica_binary().args([
        "probe",
        "--format",
        "json",
        path.to_str().unwrap(),
    ]));

    assert_exit_code_1(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("error output should be valid JSON");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error_kind"], "bad_input");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_process_of_zero_byte_file_exits_with_error() {
    let dir = test_dir();
    let input = dir.join("zero_process.mp4");
    let output_path = dir.join("zero_process_out.mp4");
    std::fs::write(&input, b"").unwrap();

    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
    ]));

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// Corrupt header (invalid magic bytes)
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_of_corrupt_header_exits_with_error() {
    let path = test_dir().join("corrupt_header.mp4");
    std::fs::write(&path, b"this is definitely not a valid media file at all").unwrap();

    let output = run_with_timeout(splica_binary().args(["probe", path.to_str().unwrap()]));

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_json_of_corrupt_header_reports_bad_input() {
    let path = test_dir().join("corrupt_header_json.mp4");
    std::fs::write(&path, b"this is definitely not a valid media file at all").unwrap();

    let output = run_with_timeout(splica_binary().args([
        "probe",
        "--format",
        "json",
        path.to_str().unwrap(),
    ]));

    assert_exit_code_1(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("error output should be valid JSON");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error_kind"], "bad_input");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_process_of_corrupt_header_exits_with_error() {
    let dir = test_dir();
    let input = dir.join("corrupt_process.mp4");
    let output_path = dir.join("corrupt_process_out.mp4");
    std::fs::write(&input, b"this is definitely not a valid media file").unwrap();

    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
    ]));

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

#[test]
fn test_that_process_json_of_corrupt_header_reports_bad_input() {
    let dir = test_dir();
    let input = dir.join("corrupt_process_json.mp4");
    let output_path = dir.join("corrupt_process_json_out.mp4");
    std::fs::write(&input, b"this is definitely not a valid media file").unwrap();

    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
        "--format",
        "json",
    ]));

    assert_exit_code_1(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("error output should be valid JSON");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error_kind"], "bad_input");

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// Truncated file (valid ftyp header, body cut off)
// ---------------------------------------------------------------------------

fn build_truncated_mp4() -> Vec<u8> {
    let mut data = Vec::new();
    // Valid ftyp box
    let ftyp_body = b"isom\x00\x00\x00\x00isom";
    let size = (8 + ftyp_body.len()) as u32;
    data.extend_from_slice(&size.to_be_bytes());
    data.extend_from_slice(b"ftyp");
    data.extend_from_slice(ftyp_body);
    // No moov box — file is truncated after ftyp
    data
}

#[test]
fn test_that_probe_of_truncated_mp4_exits_with_error() {
    let path = test_dir().join("truncated.mp4");
    std::fs::write(&path, build_truncated_mp4()).unwrap();

    let output = run_with_timeout(splica_binary().args(["probe", path.to_str().unwrap()]));

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_json_of_truncated_mp4_reports_bad_input() {
    let path = test_dir().join("truncated_json.mp4");
    std::fs::write(&path, build_truncated_mp4()).unwrap();

    let output = run_with_timeout(splica_binary().args([
        "probe",
        "--format",
        "json",
        path.to_str().unwrap(),
    ]));

    assert_exit_code_1(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("error output should be valid JSON");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error_kind"], "bad_input");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_process_of_truncated_mp4_exits_with_error() {
    let dir = test_dir();
    let input = dir.join("truncated_process.mp4");
    let output_path = dir.join("truncated_process_out.mp4");
    std::fs::write(&input, build_truncated_mp4()).unwrap();

    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
    ]));

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

// ---------------------------------------------------------------------------
// Extension mismatch (MP4 content named .webm)
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_of_mp4_named_as_webm_exits_with_error() {
    // An MP4 file with .webm extension — detect_format should identify it
    // as MP4 based on magic bytes, but the WebM demuxer would reject it.
    // Our detect_format uses magic bytes, so it should still work as MP4.
    let path = test_dir().join("mismatch.webm");
    std::fs::write(&path, build_truncated_mp4()).unwrap();

    let output = run_with_timeout(splica_binary().args(["probe", path.to_str().unwrap()]));

    // The file has valid MP4 magic but no moov — should fail with bad_input
    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_of_webm_named_as_mp4_exits_with_error() {
    // WebM EBML header in a .mp4 file — detect_format identifies it as WebM
    // based on magic bytes, then WebM demuxer tries to parse but it's truncated
    let mut data = Vec::new();
    data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]); // EBML magic
    data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08]); // size
    let path = test_dir().join("mismatch.mp4");
    std::fs::write(&path, &data).unwrap();

    let output = run_with_timeout(splica_binary().args(["probe", path.to_str().unwrap()]));

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_json_of_extension_mismatch_reports_bad_input() {
    let path = test_dir().join("mismatch_json.webm");
    std::fs::write(&path, build_truncated_mp4()).unwrap();

    let output = run_with_timeout(splica_binary().args([
        "probe",
        "--format",
        "json",
        path.to_str().unwrap(),
    ]));

    assert_exit_code_1(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("error output should be valid JSON");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error_kind"], "bad_input");

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// Truncated WebM
// ---------------------------------------------------------------------------

fn build_truncated_webm() -> Vec<u8> {
    let mut data = Vec::new();
    // EBML header magic
    data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]);
    // Minimal EBML header element (truncated — just the ID, no valid body)
    data.extend_from_slice(&[0x85]); // size = 5
    data.extend_from_slice(&[0x00; 5]); // garbage body
    data
}

#[test]
fn test_that_probe_of_truncated_webm_exits_with_error() {
    let path = test_dir().join("truncated.webm");
    std::fs::write(&path, build_truncated_webm()).unwrap();

    let output = run_with_timeout(splica_binary().args(["probe", path.to_str().unwrap()]));

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_json_of_truncated_webm_reports_bad_input() {
    let path = test_dir().join("truncated_webm_json.webm");
    std::fs::write(&path, build_truncated_webm()).unwrap();

    let output = run_with_timeout(splica_binary().args([
        "probe",
        "--format",
        "json",
        path.to_str().unwrap(),
    ]));

    assert_exit_code_1(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("error output should be valid JSON");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error_kind"], "bad_input");

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// Zero-byte .webm
// ---------------------------------------------------------------------------

#[test]
fn test_that_probe_of_zero_byte_webm_exits_with_error() {
    let path = test_dir().join("zero.webm");
    std::fs::write(&path, b"").unwrap();

    let output = run_with_timeout(splica_binary().args(["probe", path.to_str().unwrap()]));

    assert_exit_code_1(&output);
    let _ = std::fs::remove_file(&path);
}
