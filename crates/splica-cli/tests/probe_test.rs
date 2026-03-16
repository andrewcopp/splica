//! Integration tests for the `splica probe` command.

use std::io::Cursor;
use std::process::Command;

use splica_core::{Demuxer, Muxer};
use splica_mp4::{Mp4Demuxer, Mp4Muxer};

fn splica_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_splica"))
}

#[test]
fn test_that_probe_shows_help() {
    // GIVEN / WHEN
    let output = splica_binary().args(["probe", "--help"]).output().unwrap();

    // THEN
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Inspect a media file"));
    assert!(stdout.contains("--format"));
}

#[test]
fn test_that_probe_fails_on_nonexistent_file() {
    // GIVEN / WHEN
    let output = splica_binary()
        .args(["probe", "/nonexistent/path/file.mp4"])
        .output()
        .unwrap();

    // THEN
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("could not open file"));
}

#[test]
fn test_that_probe_fails_on_invalid_mp4() {
    // GIVEN — write garbage to a temp file
    let dir = std::env::temp_dir().join("splica_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("invalid.mp4");
    std::fs::write(&path, b"this is not an mp4 file").unwrap();

    // WHEN
    let output = splica_binary()
        .args(["probe", path.to_str().unwrap()])
        .output()
        .unwrap();

    // THEN
    assert!(!output.status.success());

    // Cleanup
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_text_output_shows_track_info() {
    // GIVEN — create a valid MP4 via the muxer round-trip
    let dir = std::env::temp_dir().join("splica_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("probe_test.mp4");

    // Build a valid MP4 using muxer with a fake video track
    let source_mp4 = build_muxed_test_mp4();
    std::fs::write(&path, &source_mp4).unwrap();

    // WHEN
    let output = splica_binary()
        .args(["probe", path.to_str().unwrap()])
        .output()
        .unwrap();

    // THEN
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "probe should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("Track 0"), "should show track 0");
    assert!(stdout.contains("video"), "should identify as video");
    assert!(stdout.contains("H.264"), "should identify codec as H.264");

    // Cleanup
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_probe_json_output_is_valid_json() {
    // GIVEN
    let dir = std::env::temp_dir().join("splica_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("probe_json_test.mp4");

    let source_mp4 = build_muxed_test_mp4();
    std::fs::write(&path, &source_mp4).unwrap();

    // WHEN
    let output = splica_binary()
        .args(["probe", "--format", "json", path.to_str().unwrap()])
        .output()
        .unwrap();

    // THEN
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "probe --format json should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");
    assert!(json["tracks"].is_array());
    assert_eq!(json["tracks"][0]["kind"], "video");
    assert_eq!(json["tracks"][0]["codec"], "H.264");

    // Cleanup
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_that_process_rejects_unsupported_output_format() {
    let dir = std::env::temp_dir().join("splica_test");
    std::fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("format_test_input.mp4");
    let output_path = dir.join("format_test_output.avi");

    let source_mp4 = build_muxed_test_mp4();
    std::fs::write(&input_path, &source_mp4).unwrap();

    let output = splica_binary()
        .args([
            "process",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported output format"),
        "should explain format is unsupported. stderr: {stderr}"
    );

    let _ = std::fs::remove_file(&input_path);
}

#[test]
fn test_that_process_accepts_mp4_output() {
    let dir = std::env::temp_dir().join("splica_test");
    std::fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("format_ok_input.mp4");
    let output_path = dir.join("format_ok_output.mp4");

    let source_mp4 = build_muxed_test_mp4();
    std::fs::write(&input_path, &source_mp4).unwrap();

    let output = splica_binary()
        .args([
            "process",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "mp4 output should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&output_path);
}

#[test]
fn test_that_deprecated_convert_still_works() {
    let dir = std::env::temp_dir().join("splica_test");
    std::fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("deprecated_convert_input.mp4");
    let output_path = dir.join("deprecated_convert_output.mp4");

    let source_mp4 = build_muxed_test_mp4();
    std::fs::write(&input_path, &source_mp4).unwrap();

    let output = splica_binary()
        .args([
            "convert",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "deprecated convert should still work. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("deprecated"),
        "should show deprecation warning. stderr: {stderr}"
    );

    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&output_path);
}

#[test]
#[ignore] // transcode requires re-encode; synthetic fixture's avcC is too short for H.264 decoder
fn test_that_deprecated_transcode_still_works() {
    let dir = std::env::temp_dir().join("splica_test");
    std::fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("deprecated_transcode_input.mp4");
    let output_path = dir.join("deprecated_transcode_output.mp4");

    let source_mp4 = build_muxed_test_mp4();
    std::fs::write(&input_path, &source_mp4).unwrap();

    let output = splica_binary()
        .args([
            "transcode",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "deprecated transcode should still work. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("deprecated"),
        "should show deprecation warning. stderr: {stderr}"
    );

    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&output_path);
}

/// Creates a valid MP4 by building a source, demuxing it, and remuxing it.
/// This exercises the full muxer pipeline to produce a file the demuxer can read.
fn build_muxed_test_mp4() -> Vec<u8> {
    // Build source MP4 by hand (minimal ftyp + moov + mdat)
    let source = build_source_mp4();

    // Demux it
    let mut demuxer = Mp4Demuxer::open(Cursor::new(&source)).unwrap();
    let tracks = demuxer.tracks().to_vec();

    // Remux via Mp4Muxer
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = Mp4Muxer::new(&mut output);
        for track in &tracks {
            muxer.add_track(track).unwrap();
        }
        while let Some(packet) = demuxer.read_packet().unwrap() {
            muxer.write_packet(&packet).unwrap();
        }
        muxer.finalize().unwrap();
    }

    output.into_inner()
}

/// Builds a minimal source MP4 with 1 H.264 video track and 1 sample.
fn build_source_mp4() -> Vec<u8> {
    let mut data = Vec::new();

    // ftyp box
    let ftyp = build_box(b"ftyp", &{
        let mut body = Vec::new();
        body.extend_from_slice(b"isom");
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(b"isom");
        body
    });
    data.extend_from_slice(&ftyp);

    // Sample data (fake H.264 — just 100 bytes of zeros)
    let sample_data = vec![0u8; 100];
    let mdat_offset = data.len() as u64;
    let mdat = build_box(b"mdat", &sample_data);
    let sample_offset = mdat_offset + 8; // mdat header is 8 bytes
    data.extend_from_slice(&mdat);

    // moov box
    let moov_body = build_moov(320, 240, sample_offset, sample_data.len() as u32);
    let moov = build_box(b"moov", &moov_body);
    data.extend_from_slice(&moov);

    data
}

fn build_box(fourcc: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = (8 + body.len()) as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(fourcc);
    out.extend_from_slice(body);
    out
}

fn build_full_box(fourcc: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut full_body = Vec::with_capacity(4 + body.len());
    full_body.push(version);
    let flag_bytes = flags.to_be_bytes();
    full_body.extend_from_slice(&flag_bytes[1..4]);
    full_body.extend_from_slice(body);
    build_box(fourcc, &full_body)
}

fn build_moov(width: u16, height: u16, sample_offset: u64, sample_size: u32) -> Vec<u8> {
    let mut moov = Vec::new();

    // mvhd
    let mvhd = {
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
        body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
        body.extend_from_slice(&30000u32.to_be_bytes()); // timescale
        body.extend_from_slice(&30030u32.to_be_bytes()); // duration (1.001s)
        body.extend_from_slice(&0x00010000u32.to_be_bytes()); // rate = 1.0
        body.extend_from_slice(&0x0100u16.to_be_bytes()); // volume = 1.0
        body.extend_from_slice(&[0u8; 10]); // reserved
                                            // Identity matrix (36 bytes)
        for &val in &[0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
            body.extend_from_slice(&val.to_be_bytes());
        }
        body.extend_from_slice(&[0u8; 24]); // pre_defined
        body.extend_from_slice(&2u32.to_be_bytes()); // next_track_ID
        build_full_box(b"mvhd", 0, 0, &body)
    };
    moov.extend_from_slice(&mvhd);

    // trak
    let trak = build_video_trak(width, height, sample_offset, sample_size);
    moov.extend_from_slice(&trak);

    moov
}

fn build_video_trak(width: u16, height: u16, sample_offset: u64, sample_size: u32) -> Vec<u8> {
    let mut trak_body = Vec::new();

    // tkhd
    let tkhd = {
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
        body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
        body.extend_from_slice(&1u32.to_be_bytes()); // track_id
        body.extend_from_slice(&0u32.to_be_bytes()); // reserved
        body.extend_from_slice(&30030u32.to_be_bytes()); // duration
        body.extend_from_slice(&[0u8; 8]); // reserved
        body.extend_from_slice(&0u16.to_be_bytes()); // layer
        body.extend_from_slice(&0u16.to_be_bytes()); // alternate_group
        body.extend_from_slice(&0u16.to_be_bytes()); // volume
        body.extend_from_slice(&[0u8; 2]); // reserved
                                           // Identity matrix
        for &val in &[0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
            body.extend_from_slice(&val.to_be_bytes());
        }
        body.extend_from_slice(&((width as u32) << 16).to_be_bytes());
        body.extend_from_slice(&((height as u32) << 16).to_be_bytes());
        build_full_box(b"tkhd", 0, 3, &body)
    };
    trak_body.extend_from_slice(&tkhd);

    // mdia
    let mdia = build_mdia(width, height, sample_offset, sample_size);
    trak_body.extend_from_slice(&mdia);

    build_box(b"trak", &trak_body)
}

fn build_mdia(width: u16, height: u16, sample_offset: u64, sample_size: u32) -> Vec<u8> {
    let mut mdia_body = Vec::new();

    // mdhd
    let mdhd = {
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
        body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
        body.extend_from_slice(&30000u32.to_be_bytes()); // timescale
        body.extend_from_slice(&1001u32.to_be_bytes()); // duration (1 frame at 29.97fps)
        body.extend_from_slice(&0u16.to_be_bytes()); // language
        body.extend_from_slice(&0u16.to_be_bytes()); // pre_defined
        build_full_box(b"mdhd", 0, 0, &body)
    };
    mdia_body.extend_from_slice(&mdhd);

    // hdlr
    let hdlr = {
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
        body.extend_from_slice(b"vide"); // handler_type
        body.extend_from_slice(&[0u8; 12]); // reserved
        body.push(0); // name (null terminated)
        build_full_box(b"hdlr", 0, 0, &body)
    };
    mdia_body.extend_from_slice(&hdlr);

    // minf
    let minf = build_minf(width, height, sample_offset, sample_size);
    mdia_body.extend_from_slice(&minf);

    build_box(b"mdia", &mdia_body)
}

fn build_minf(width: u16, height: u16, sample_offset: u64, sample_size: u32) -> Vec<u8> {
    let mut minf_body = Vec::new();

    // stbl
    let stbl = build_stbl(width, height, sample_offset, sample_size);
    minf_body.extend_from_slice(&stbl);

    build_box(b"minf", &minf_body)
}

fn build_stbl(width: u16, height: u16, sample_offset: u64, sample_size: u32) -> Vec<u8> {
    let mut stbl_body = Vec::new();

    // stsd (sample description — avc1 entry)
    let stsd = {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes()); // entry_count

        // avc1 visual sample entry
        let avc1_entry = {
            let mut entry = Vec::new();
            // 6 reserved + 2 data_ref_index
            entry.extend_from_slice(&[0u8; 6]);
            entry.extend_from_slice(&1u16.to_be_bytes());
            // pre_defined + reserved (16 bytes)
            entry.extend_from_slice(&[0u8; 16]);
            entry.extend_from_slice(&width.to_be_bytes());
            entry.extend_from_slice(&height.to_be_bytes());
            // horiz_res, vert_res
            entry.extend_from_slice(&0x00480000u32.to_be_bytes());
            entry.extend_from_slice(&0x00480000u32.to_be_bytes());
            entry.extend_from_slice(&0u32.to_be_bytes()); // reserved
            entry.extend_from_slice(&1u16.to_be_bytes()); // frame_count
            entry.extend_from_slice(&[0u8; 32]); // compressor_name
            entry.extend_from_slice(&0x0018u16.to_be_bytes()); // depth
            entry.extend_from_slice(&(-1i16).to_be_bytes()); // pre_defined

            // avcC sub-box (minimal)
            let avcc_data = vec![
                1, 0x42, 0xC0, 0x1E, 0xFF, // version, profile, compat, level, nal_len=4
                0xE1, // num_sps = 1
                0x00, 0x04, // sps_length
                0x67, 0x42, 0xC0, 0x1E, // SPS (fake but parseable)
                0x01, // num_pps = 1
                0x00, 0x02, // pps_length
                0x68, 0xCE, // PPS (fake)
            ];
            let avcc_box = build_box(b"avcC", &avcc_data);
            entry.extend_from_slice(&avcc_box);

            entry
        };

        let avc1_box = build_box(b"avc1", &avc1_entry);
        body.extend_from_slice(&avc1_box);

        build_full_box(b"stsd", 0, 0, &body)
    };
    stbl_body.extend_from_slice(&stsd);

    // stts (1 entry: 1 sample, delta=1001)
    let stts = {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        body.extend_from_slice(&1u32.to_be_bytes()); // sample_count
        body.extend_from_slice(&1001u32.to_be_bytes()); // sample_delta
        build_full_box(b"stts", 0, 0, &body)
    };
    stbl_body.extend_from_slice(&stts);

    // stsc (1 entry: first_chunk=1, samples_per_chunk=1, desc_idx=1)
    let stsc = {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes());
        build_full_box(b"stsc", 0, 0, &body)
    };
    stbl_body.extend_from_slice(&stsc);

    // stsz (1 sample)
    let stsz = {
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_be_bytes()); // sample_size (0 = per-sample)
        body.extend_from_slice(&1u32.to_be_bytes()); // sample_count
        body.extend_from_slice(&sample_size.to_be_bytes());
        build_full_box(b"stsz", 0, 0, &body)
    };
    stbl_body.extend_from_slice(&stsz);

    // stco (1 chunk offset)
    let stco = {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        body.extend_from_slice(&(sample_offset as u32).to_be_bytes());
        build_full_box(b"stco", 0, 0, &body)
    };
    stbl_body.extend_from_slice(&stco);

    // stss (1 sync sample)
    let stss = {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        body.extend_from_slice(&1u32.to_be_bytes()); // sample_number (1-based)
        build_full_box(b"stss", 0, 0, &body)
    };
    stbl_body.extend_from_slice(&stss);

    build_box(b"stbl", &stbl_body)
}
