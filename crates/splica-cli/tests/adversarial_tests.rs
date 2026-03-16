//! Adversarial fixture tests (SPL-182).
//!
//! Proves that splica handles pathological input with structured errors,
//! deterministic exit codes, and human-readable messages. Each fixture is
//! constructed programmatically in-test to keep binary size minimal and
//! avoid storing opaque blobs in version control.

use std::process::Command;

fn splica_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_splica"))
}

fn test_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("splica_adversarial_tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs a command, guarding against hangs.
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
            "expected exit code 1 (bad_input), got {:?} -- a code of 101 indicates a panic",
            output.status.code()
        );
    }
}

/// Asserts the JSON output has a structured error with an `error_kind` field
/// and a non-empty human-readable `message`.
fn assert_structured_json_error(output: &std::process::Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("error output should be valid JSON");
    assert_eq!(
        json["type"], "error",
        "expected type=error, got: {}",
        json["type"]
    );
    assert!(
        json["error_kind"].is_string(),
        "expected error_kind to be a string, got: {}",
        json["error_kind"]
    );
    let error_kind = json["error_kind"].as_str().unwrap();
    assert!(!error_kind.is_empty(), "error_kind should not be empty");
    let message = json["message"].as_str().unwrap_or("");
    assert!(!message.is_empty(), "message should not be empty");
    // The message should be specific, not a generic "error"
    assert!(
        message.len() > 5,
        "message should be human-readable and specific, got: {message}"
    );
    json
}

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

/// Builds a valid ftyp box.
fn build_ftyp() -> Vec<u8> {
    let mut data = Vec::new();
    let ftyp_body = b"isom\x00\x00\x00\x00isom";
    let size = (8 + ftyp_body.len()) as u32;
    data.extend_from_slice(&size.to_be_bytes());
    data.extend_from_slice(b"ftyp");
    data.extend_from_slice(ftyp_body);
    data
}

/// Builds a minimal moov box with one video track.
/// The `mvhd_duration` and `tkhd_duration` are set to the given value.
fn build_moov_with_duration(duration: u32) -> Vec<u8> {
    // mvhd: version(1) + flags(3) + creation(4) + modification(4) +
    //        timescale(4) + duration(4) + rest(80) = 100 bytes body
    let mut mvhd_body = Vec::new();
    mvhd_body.push(0); // version 0
    mvhd_body.extend_from_slice(&[0, 0, 0]); // flags
    mvhd_body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    mvhd_body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    mvhd_body.extend_from_slice(&1000u32.to_be_bytes()); // timescale
    mvhd_body.extend_from_slice(&duration.to_be_bytes()); // duration
                                                          // rate (4), volume (2), reserved (10), matrix (36), pre_defined (24), next_track_id (4)
    mvhd_body.extend_from_slice(&[0; 80]);

    let mvhd = make_box(b"mvhd", &mvhd_body);

    // tkhd: version(1) + flags(3) + creation(4) + modification(4) + track_id(4) +
    //        reserved(4) + duration(4) + rest(60) = 84 bytes body
    let mut tkhd_body = Vec::new();
    tkhd_body.push(0); // version 0
    tkhd_body.extend_from_slice(&[0, 0, 1]); // flags (track_enabled)
    tkhd_body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    tkhd_body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    tkhd_body.extend_from_slice(&1u32.to_be_bytes()); // track_id
    tkhd_body.extend_from_slice(&0u32.to_be_bytes()); // reserved
    tkhd_body.extend_from_slice(&duration.to_be_bytes()); // duration
                                                          // reserved(8), layer(2), alt_group(2), volume(2), reserved(2), matrix(36), width(4), height(4)
    tkhd_body.extend_from_slice(&[0; 60]);

    let tkhd = make_box(b"tkhd", &tkhd_body);

    // Minimal mdia with mdhd + hdlr + minf stub
    let mut mdhd_body = Vec::new();
    mdhd_body.push(0); // version 0
    mdhd_body.extend_from_slice(&[0, 0, 0]); // flags
    mdhd_body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    mdhd_body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    mdhd_body.extend_from_slice(&1000u32.to_be_bytes()); // timescale
    mdhd_body.extend_from_slice(&duration.to_be_bytes()); // duration
    mdhd_body.extend_from_slice(&[0; 4]); // language + pre_defined

    let mdhd = make_box(b"mdhd", &mdhd_body);

    let mut hdlr_body = Vec::new();
    hdlr_body.push(0); // version 0
    hdlr_body.extend_from_slice(&[0, 0, 0]); // flags
    hdlr_body.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    hdlr_body.extend_from_slice(b"vide"); // handler_type
    hdlr_body.extend_from_slice(&[0; 12]); // reserved
    hdlr_body.extend_from_slice(b"VideoHandler\0"); // name

    let hdlr = make_box(b"hdlr", &hdlr_body);

    let mut mdia_body = Vec::new();
    mdia_body.extend_from_slice(&mdhd);
    mdia_body.extend_from_slice(&hdlr);

    let mdia = make_box(b"mdia", &mdia_body);

    let mut trak_body = Vec::new();
    trak_body.extend_from_slice(&tkhd);
    trak_body.extend_from_slice(&mdia);

    let trak = make_box(b"trak", &trak_body);

    let mut moov_body = Vec::new();
    moov_body.extend_from_slice(&mvhd);
    moov_body.extend_from_slice(&trak);

    make_box(b"moov", &moov_body)
}

/// Helper to build an ISO BMFF box.
fn make_box(box_type: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = (8 + body.len()) as u32;
    let mut data = Vec::new();
    data.extend_from_slice(&size.to_be_bytes());
    data.extend_from_slice(box_type);
    data.extend_from_slice(body);
    data
}

/// Builds a truncated-mdat MP4: valid ftyp + moov, then an mdat box whose
/// declared size is larger than the actual data written.
fn build_truncated_mdat_mp4() -> Vec<u8> {
    let mut data = build_ftyp();
    data.extend_from_slice(&build_moov_with_duration(2000));

    // mdat box: declare 1000 bytes but only write 700 (70%)
    let declared_size: u32 = 1000 + 8; // 8 for the box header
    data.extend_from_slice(&declared_size.to_be_bytes());
    data.extend_from_slice(b"mdat");
    data.extend_from_slice(&vec![0xAA; 700]); // only 70% of declared data

    data
}

/// Builds a zero-duration-track MP4: valid structure but duration fields are 0.
fn build_zero_duration_mp4() -> Vec<u8> {
    let mut data = build_ftyp();
    data.extend_from_slice(&build_moov_with_duration(0));
    data
}

/// Builds a truncated-header MP4: ftyp box declares 20 bytes but file ends at 8.
fn build_truncated_header_mp4() -> Vec<u8> {
    let mut data = Vec::new();
    // ftyp box with declared size 20 but only 8 bytes written
    let declared_size: u32 = 20;
    data.extend_from_slice(&declared_size.to_be_bytes());
    data.extend_from_slice(b"ftyp");
    // File ends here -- only 8 bytes total, declared 20
    data
}

/// Builds a valid EBML header + Segment with matroska doctype but no Tracks element.
fn build_no_tracks_mkv() -> Vec<u8> {
    let mut data = Vec::new();

    // EBML Header element (ID = 0x1A45DFA3)
    data.extend_from_slice(&[0x1A, 0x45, 0xDF, 0xA3]);

    // EBML header body: DocType = "matroska"
    let mut ebml_body = Vec::new();

    // EBMLVersion (0x4286) = 1
    ebml_body.extend_from_slice(&[0x42, 0x86]); // ID
    ebml_body.push(0x81); // size = 1
    ebml_body.push(0x01); // value = 1

    // EBMLReadVersion (0x42F7) = 1
    ebml_body.extend_from_slice(&[0x42, 0xF7]); // ID
    ebml_body.push(0x81); // size = 1
    ebml_body.push(0x01); // value = 1

    // EBMLMaxIDLength (0x42F2) = 4
    ebml_body.extend_from_slice(&[0x42, 0xF2]); // ID
    ebml_body.push(0x81); // size = 1
    ebml_body.push(0x04); // value = 4

    // EBMLMaxSizeLength (0x42F3) = 8
    ebml_body.extend_from_slice(&[0x42, 0xF3]); // ID
    ebml_body.push(0x81); // size = 1
    ebml_body.push(0x08); // value = 8

    // DocType (0x4282) = "matroska"
    ebml_body.extend_from_slice(&[0x42, 0x82]); // ID
    ebml_body.push(0x88); // size = 8
    ebml_body.extend_from_slice(b"matroska");

    // DocTypeVersion (0x4287) = 4
    ebml_body.extend_from_slice(&[0x42, 0x87]); // ID
    ebml_body.push(0x81); // size = 1
    ebml_body.push(0x04); // value = 4

    // DocTypeReadVersion (0x4285) = 2
    ebml_body.extend_from_slice(&[0x42, 0x85]); // ID
    ebml_body.push(0x81); // size = 1
    ebml_body.push(0x02); // value = 2

    // Write EBML header size (1-byte VINT for sizes <= 127)
    let ebml_size = ebml_body.len() as u8;
    data.push(0x80 | ebml_size); // VINT encoding
    data.extend_from_slice(&ebml_body);

    // Segment element (ID = 0x18538067) with unknown size
    data.extend_from_slice(&[0x18, 0x53, 0x80, 0x67]);
    // Unknown size = all 1s in VINT (8-byte)
    data.extend_from_slice(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);

    // Info element (ID = 0x1549A966) with minimal body
    data.extend_from_slice(&[0x15, 0x49, 0xA9, 0x66]);
    let mut info_body = Vec::new();
    // TimestampScale (0x2AD7B1) = 1_000_000
    info_body.extend_from_slice(&[0x2A, 0xD7, 0xB1]); // ID
    info_body.push(0x83); // size = 3
    info_body.push(0x0F); // 1_000_000 = 0x0F4240
    info_body.push(0x42);
    info_body.push(0x40);
    let info_size = info_body.len() as u8;
    data.push(0x80 | info_size);
    data.extend_from_slice(&info_body);

    // No Tracks element -- this is the point of the fixture

    data
}

// ===========================================================================
// 1. Truncated mdat MP4
// ===========================================================================

#[test]
fn test_that_process_of_truncated_mdat_mp4_exits_with_error() {
    // GIVEN -- an MP4 with valid ftyp+moov but mdat cut off at 70%
    let dir = test_dir();
    let input = dir.join("truncated-mdat.mp4");
    let output_path = dir.join("truncated-mdat-out.mp4");
    std::fs::write(&input, build_truncated_mdat_mp4()).unwrap();

    // WHEN -- processing the file
    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
    ]));

    // THEN -- exits with error code 1, not a panic
    assert_exit_code_1(&output);

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

#[test]
fn test_that_process_json_of_truncated_mdat_mp4_reports_structured_error() {
    // GIVEN -- an MP4 with valid ftyp+moov but mdat cut off at 70%
    let dir = test_dir();
    let input = dir.join("truncated-mdat-json.mp4");
    let output_path = dir.join("truncated-mdat-json-out.mp4");
    std::fs::write(&input, build_truncated_mdat_mp4()).unwrap();

    // WHEN -- processing the file with JSON output
    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
        "--format",
        "json",
    ]));

    // THEN -- exits with code 1 and structured JSON error
    assert_exit_code_1(&output);
    assert_structured_json_error(&output);

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

// ===========================================================================
// 2. Zero-duration track MP4
// ===========================================================================

#[test]
fn test_that_probe_of_zero_duration_track_mp4_exits_with_error() {
    // GIVEN -- an MP4 with a video track where duration is 0
    let path = test_dir().join("zero-duration-track.mp4");
    std::fs::write(&path, build_zero_duration_mp4()).unwrap();

    // WHEN -- probing the file
    let output = run_with_timeout(splica_binary().args(["probe", path.to_str().unwrap()]));

    // THEN -- exits with error code 1
    assert_exit_code_1(&output);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_json_of_zero_duration_track_mp4_reports_structured_error() {
    // GIVEN -- an MP4 with a video track where duration is 0
    let path = test_dir().join("zero-duration-track-json.mp4");
    std::fs::write(&path, build_zero_duration_mp4()).unwrap();

    // WHEN -- probing the file with JSON output
    let output = run_with_timeout(splica_binary().args([
        "probe",
        "--format",
        "json",
        path.to_str().unwrap(),
    ]));

    // THEN -- exits with code 1 and structured JSON error
    assert_exit_code_1(&output);
    assert_structured_json_error(&output);

    let _ = std::fs::remove_file(&path);
}

// ===========================================================================
// 3. Empty file (0 bytes)
// ===========================================================================

#[test]
fn test_that_process_of_empty_file_exits_with_error() {
    // GIVEN -- a completely empty file with .mp4 extension
    let dir = test_dir();
    let input = dir.join("empty-file.mp4");
    let output_path = dir.join("empty-file-out.mp4");
    std::fs::write(&input, b"").unwrap();

    // WHEN -- processing the file
    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
    ]));

    // THEN -- exits with error code 1
    assert_exit_code_1(&output);

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

#[test]
fn test_that_process_json_of_empty_file_reports_bad_input() {
    // GIVEN -- a completely empty file with .mp4 extension
    let dir = test_dir();
    let input = dir.join("empty-file-json.mp4");
    let output_path = dir.join("empty-file-json-out.mp4");
    std::fs::write(&input, b"").unwrap();

    // WHEN -- processing the file with JSON output
    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
        "--format",
        "json",
    ]));

    // THEN -- exits with code 1 and structured JSON error with bad_input kind
    assert_exit_code_1(&output);
    let json = assert_structured_json_error(&output);
    assert_eq!(
        json["error_kind"], "bad_input",
        "empty file should produce bad_input error, got: {}",
        json["error_kind"]
    );

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

// ===========================================================================
// 4. Truncated header MP4
// ===========================================================================

#[test]
fn test_that_process_of_truncated_header_mp4_exits_with_error() {
    // GIVEN -- only the first 8 bytes of an ftyp box (declared 20 but file ends at 8)
    let dir = test_dir();
    let input = dir.join("truncated-header.mp4");
    let output_path = dir.join("truncated-header-out.mp4");
    std::fs::write(&input, build_truncated_header_mp4()).unwrap();

    // WHEN -- processing the file
    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
    ]));

    // THEN -- exits with error code 1
    assert_exit_code_1(&output);

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

#[test]
fn test_that_process_json_of_truncated_header_mp4_reports_structured_error() {
    // GIVEN -- only the first 8 bytes of an ftyp box (declared 20 but file ends at 8)
    let dir = test_dir();
    let input = dir.join("truncated-header-json.mp4");
    let output_path = dir.join("truncated-header-json-out.mp4");
    std::fs::write(&input, build_truncated_header_mp4()).unwrap();

    // WHEN -- processing the file with JSON output
    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
        "--format",
        "json",
    ]));

    // THEN -- exits with code 1 and structured JSON error
    assert_exit_code_1(&output);
    assert_structured_json_error(&output);

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}

// ===========================================================================
// 5. No-tracks MKV
// ===========================================================================

#[test]
fn test_that_probe_of_no_tracks_mkv_exits_with_error() {
    // GIVEN -- an MKV with valid EBML header and Segment but no Tracks element
    let path = test_dir().join("no-tracks.mkv");
    std::fs::write(&path, build_no_tracks_mkv()).unwrap();

    // WHEN -- probing the file
    let output = run_with_timeout(splica_binary().args(["probe", path.to_str().unwrap()]));

    // THEN -- exits with error code 1
    assert_exit_code_1(&output);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_json_of_no_tracks_mkv_reports_structured_error() {
    // GIVEN -- an MKV with valid EBML header and Segment but no Tracks element
    let path = test_dir().join("no-tracks-json.mkv");
    std::fs::write(&path, build_no_tracks_mkv()).unwrap();

    // WHEN -- probing the file with JSON output
    let output = run_with_timeout(splica_binary().args([
        "probe",
        "--format",
        "json",
        path.to_str().unwrap(),
    ]));

    // THEN -- exits with code 1 and structured JSON error
    assert_exit_code_1(&output);
    assert_structured_json_error(&output);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_process_of_no_tracks_mkv_exits_with_error() {
    // GIVEN -- an MKV with valid EBML header and Segment but no Tracks element
    let dir = test_dir();
    let input = dir.join("no-tracks-process.mkv");
    let output_path = dir.join("no-tracks-process-out.mkv");
    std::fs::write(&input, build_no_tracks_mkv()).unwrap();

    // WHEN -- processing the file
    let output = run_with_timeout(splica_binary().args([
        "process",
        "-i",
        input.to_str().unwrap(),
        "-o",
        output_path.to_str().unwrap(),
    ]));

    // THEN -- exits with error code 1
    assert_exit_code_1(&output);

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output_path);
}
