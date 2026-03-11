//! Integration tests for the `splica trim` command.
//!
//! Validates that trimmed output starts on a keyframe (SPL-39).

use std::io::Cursor;

use splica_core::{Demuxer, Muxer};
use splica_mp4::{Mp4Demuxer, Mp4Muxer};

fn splica_binary() -> std::process::Command {
    std::process::Command::new(env!("CARGO_BIN_EXE_splica"))
}

/// Builds a minimal MP4 with multiple samples and specific keyframe placement.
///
/// Layout: 10 samples at 30fps (timescale=30000, delta=1001 per sample ≈ 29.97fps)
/// Keyframes at samples 1 and 6 (i.e., at t≈0.0s and t≈0.167s)
/// This lets us test trimming at a non-keyframe boundary.
fn build_multi_sample_mp4() -> Vec<u8> {
    let num_samples: u32 = 10;
    let sample_size: u32 = 50;
    let sample_data = vec![0xAAu8; sample_size as usize];

    let mut data = Vec::new();

    // ftyp
    let ftyp_body = {
        let mut body = Vec::new();
        body.extend_from_slice(b"isom");
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(b"isom");
        body
    };
    data.extend_from_slice(&build_box(b"ftyp", &ftyp_body));

    // mdat — all samples concatenated
    let mdat_offset = data.len() as u64;
    let mut mdat_body = Vec::new();
    for _ in 0..num_samples {
        mdat_body.extend_from_slice(&sample_data);
    }
    let first_sample_offset = mdat_offset + 8; // mdat header
    data.extend_from_slice(&build_box(b"mdat", &mdat_body));

    // moov
    let moov = build_multi_sample_moov(320, 240, first_sample_offset, sample_size, num_samples);
    data.extend_from_slice(&build_box(b"moov", &moov));

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

fn build_multi_sample_moov(
    width: u16,
    height: u16,
    first_offset: u64,
    sample_size: u32,
    num_samples: u32,
) -> Vec<u8> {
    let timescale: u32 = 30000;
    let sample_delta: u32 = 1001;
    let total_duration = num_samples * sample_delta;

    let mut moov = Vec::new();

    // mvhd
    let mvhd = {
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_be_bytes()); // creation_time
        body.extend_from_slice(&0u32.to_be_bytes()); // modification_time
        body.extend_from_slice(&timescale.to_be_bytes());
        body.extend_from_slice(&total_duration.to_be_bytes());
        body.extend_from_slice(&0x00010000u32.to_be_bytes()); // rate
        body.extend_from_slice(&0x0100u16.to_be_bytes()); // volume
        body.extend_from_slice(&[0u8; 10]); // reserved
        for &val in &[0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
            body.extend_from_slice(&val.to_be_bytes());
        }
        body.extend_from_slice(&[0u8; 24]); // pre_defined
        body.extend_from_slice(&2u32.to_be_bytes()); // next_track_ID
        build_full_box(b"mvhd", 0, 0, &body)
    };
    moov.extend_from_slice(&mvhd);

    // trak
    let trak = build_multi_sample_trak(
        width,
        height,
        first_offset,
        sample_size,
        num_samples,
        timescale,
    );
    moov.extend_from_slice(&trak);

    moov
}

fn build_multi_sample_trak(
    width: u16,
    height: u16,
    first_offset: u64,
    sample_size: u32,
    num_samples: u32,
    timescale: u32,
) -> Vec<u8> {
    let total_duration = num_samples * 1001;
    let mut trak_body = Vec::new();

    // tkhd
    let tkhd = {
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes()); // track_id
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(&total_duration.to_be_bytes());
        body.extend_from_slice(&[0u8; 8]);
        body.extend_from_slice(&0u16.to_be_bytes());
        body.extend_from_slice(&0u16.to_be_bytes());
        body.extend_from_slice(&0u16.to_be_bytes());
        body.extend_from_slice(&[0u8; 2]);
        for &val in &[0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
            body.extend_from_slice(&val.to_be_bytes());
        }
        body.extend_from_slice(&((width as u32) << 16).to_be_bytes());
        body.extend_from_slice(&((height as u32) << 16).to_be_bytes());
        build_full_box(b"tkhd", 0, 3, &body)
    };
    trak_body.extend_from_slice(&tkhd);

    // mdia
    let mdia = {
        let mut mdia_body = Vec::new();

        // mdhd
        let mdhd = {
            let mut body = Vec::new();
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(&timescale.to_be_bytes());
            body.extend_from_slice(&total_duration.to_be_bytes());
            body.extend_from_slice(&0u16.to_be_bytes());
            body.extend_from_slice(&0u16.to_be_bytes());
            build_full_box(b"mdhd", 0, 0, &body)
        };
        mdia_body.extend_from_slice(&mdhd);

        // hdlr
        let hdlr = {
            let mut body = Vec::new();
            body.extend_from_slice(&0u32.to_be_bytes());
            body.extend_from_slice(b"vide");
            body.extend_from_slice(&[0u8; 12]);
            body.push(0);
            build_full_box(b"hdlr", 0, 0, &body)
        };
        mdia_body.extend_from_slice(&hdlr);

        // minf → stbl
        let stbl = build_multi_sample_stbl(width, height, first_offset, sample_size, num_samples);
        let minf = build_box(b"minf", &build_box(b"stbl", &stbl));
        mdia_body.extend_from_slice(&minf);

        build_box(b"mdia", &mdia_body)
    };
    trak_body.extend_from_slice(&mdia);

    build_box(b"trak", &trak_body)
}

fn build_multi_sample_stbl(
    width: u16,
    height: u16,
    first_offset: u64,
    sample_size: u32,
    num_samples: u32,
) -> Vec<u8> {
    let mut stbl = Vec::new();

    // stsd — avc1 entry
    let stsd = {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes());

        let avc1_entry = {
            let mut entry = Vec::new();
            entry.extend_from_slice(&[0u8; 6]);
            entry.extend_from_slice(&1u16.to_be_bytes());
            entry.extend_from_slice(&[0u8; 16]);
            entry.extend_from_slice(&width.to_be_bytes());
            entry.extend_from_slice(&height.to_be_bytes());
            entry.extend_from_slice(&0x00480000u32.to_be_bytes());
            entry.extend_from_slice(&0x00480000u32.to_be_bytes());
            entry.extend_from_slice(&0u32.to_be_bytes());
            entry.extend_from_slice(&1u16.to_be_bytes());
            entry.extend_from_slice(&[0u8; 32]);
            entry.extend_from_slice(&0x0018u16.to_be_bytes());
            entry.extend_from_slice(&(-1i16).to_be_bytes());

            let avcc_data = vec![
                1, 0x42, 0xC0, 0x1E, 0xFF, 0xE1, 0x00, 0x04, 0x67, 0x42, 0xC0, 0x1E, 0x01, 0x00,
                0x02, 0x68, 0xCE,
            ];
            entry.extend_from_slice(&build_box(b"avcC", &avcc_data));
            entry
        };
        body.extend_from_slice(&build_box(b"avc1", &avc1_entry));
        build_full_box(b"stsd", 0, 0, &body)
    };
    stbl.extend_from_slice(&stsd);

    // stts — all samples have the same delta
    let stts = {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&num_samples.to_be_bytes());
        body.extend_from_slice(&1001u32.to_be_bytes());
        build_full_box(b"stts", 0, 0, &body)
    };
    stbl.extend_from_slice(&stts);

    // stsc — 1 chunk with all samples
    let stsc = {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&num_samples.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes());
        build_full_box(b"stsc", 0, 0, &body)
    };
    stbl.extend_from_slice(&stsc);

    // stsz — all samples same size
    let stsz = {
        let mut body = Vec::new();
        body.extend_from_slice(&sample_size.to_be_bytes()); // uniform size
        body.extend_from_slice(&num_samples.to_be_bytes());
        build_full_box(b"stsz", 0, 0, &body)
    };
    stbl.extend_from_slice(&stsz);

    // stco — 1 chunk
    let stco = {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&(first_offset as u32).to_be_bytes());
        build_full_box(b"stco", 0, 0, &body)
    };
    stbl.extend_from_slice(&stco);

    // stss — keyframes at samples 1 and 6 (1-based)
    let stss = {
        let mut body = Vec::new();
        body.extend_from_slice(&2u32.to_be_bytes()); // 2 sync samples
        body.extend_from_slice(&1u32.to_be_bytes()); // sample 1
        body.extend_from_slice(&6u32.to_be_bytes()); // sample 6
        build_full_box(b"stss", 0, 0, &body)
    };
    stbl.extend_from_slice(&stss);

    stbl
}

/// Creates a demux-remux round-trip MP4 from the multi-sample source.
fn build_muxed_multi_sample_mp4() -> Vec<u8> {
    let source = build_multi_sample_mp4();
    let mut demuxer = Mp4Demuxer::open(Cursor::new(&source)).unwrap();
    let tracks = demuxer.tracks().to_vec();

    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = Mp4Muxer::new(&mut output);
        for track in &tracks {
            let track_idx = track.index;
            if let (Some(config), Some(ts)) = (
                demuxer.codec_config(track_idx).cloned(),
                demuxer.track_timescale(track_idx),
            ) {
                muxer.add_track_with_config(track, config, ts).unwrap();
            } else {
                muxer.add_track(track).unwrap();
            }
        }
        let metadata = demuxer.metadata().to_vec();
        muxer.set_metadata(metadata);

        while let Some(packet) = demuxer.read_packet().unwrap() {
            muxer.write_packet_data(&packet).unwrap();
        }
        muxer.finalize_file().unwrap();
    }

    output.into_inner()
}

#[test]
fn test_that_trim_output_starts_on_keyframe() {
    // GIVEN — MP4 with keyframes at samples 1 (t=0) and 6 (t≈0.167s)
    let dir = std::env::temp_dir().join("splica_test_trim");
    std::fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("trim_input.mp4");
    let output_path = dir.join("trim_output.mp4");

    let mp4_data = build_muxed_multi_sample_mp4();
    std::fs::write(&input_path, &mp4_data).unwrap();

    // WHEN — trim starting at 0.1s (between keyframe 1 at t=0 and keyframe 6 at t≈0.167s)
    let output = splica_binary()
        .args([
            "trim",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--start",
            "0.1",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "trim should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — the output should exist and the first packet should be a keyframe
    let mut demuxer = Mp4Demuxer::open(std::fs::File::open(&output_path).unwrap()).unwrap();
    let first_packet = demuxer.read_packet().unwrap();
    assert!(first_packet.is_some(), "output should have packets");
    assert!(
        first_packet.unwrap().is_keyframe,
        "first packet in trimmed output must be a keyframe"
    );

    // Cleanup
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&output_path);
}

#[test]
fn test_that_trim_without_start_includes_all_packets() {
    // GIVEN
    let dir = std::env::temp_dir().join("splica_test_trim");
    std::fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("trim_nostart_input.mp4");
    let output_path = dir.join("trim_nostart_output.mp4");

    let mp4_data = build_muxed_multi_sample_mp4();
    std::fs::write(&input_path, &mp4_data).unwrap();

    // WHEN — trim with only --end
    let output = splica_binary()
        .args([
            "trim",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--end",
            "0.2",
        ])
        .output()
        .unwrap();

    // THEN
    assert!(
        output.status.success(),
        "trim should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut demuxer = Mp4Demuxer::open(std::fs::File::open(&output_path).unwrap()).unwrap();
    let mut count = 0;
    while demuxer.read_packet().unwrap().is_some() {
        count += 1;
    }
    assert!(count > 0, "output should have packets");

    // Cleanup
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&output_path);
}

#[test]
fn test_that_trim_json_output_includes_packet_counts() {
    // GIVEN
    let dir = std::env::temp_dir().join("splica_test_trim");
    std::fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("trim_json_input.mp4");
    let output_path = dir.join("trim_json_output.mp4");

    let mp4_data = build_muxed_multi_sample_mp4();
    std::fs::write(&input_path, &mp4_data).unwrap();

    // WHEN — trim with --format json
    let output = splica_binary()
        .args([
            "trim",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--start",
            "0.1",
            "--end",
            "0.3",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "trim --format json should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // THEN — stdout should be valid JSON with expected fields
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected valid JSON, got error {e}: {stdout}"));

    assert!(json["packets_written"].as_u64().unwrap() > 0);
    assert!(json["packets_skipped"].as_u64().unwrap() > 0);
    assert!(json["actual_start_seconds"].is_number());
    assert!(json["actual_end_seconds"].is_number());
    assert!(json["input"].as_str().is_some());
    assert!(json["output"].as_str().is_some());

    // Cleanup
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&output_path);
}

#[test]
fn test_that_trim_json_error_reports_bad_input() {
    // GIVEN — a corrupt file
    let dir = std::env::temp_dir().join("splica_test_trim");
    std::fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("trim_json_corrupt.mp4");
    let output_path = dir.join("trim_json_corrupt_out.mp4");
    std::fs::write(&input_path, b"not a valid media file").unwrap();

    // WHEN — trim with --format json on corrupt input
    let output = splica_binary()
        .args([
            "trim",
            "-i",
            input_path.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    // THEN — should fail with structured JSON error
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected valid JSON error, got error {e}: {stdout}"));

    assert_eq!(json["type"], "error");
    assert_eq!(json["error_kind"], "bad_input");
    assert!(json["message"].as_str().is_some());

    // Cleanup
    let _ = std::fs::remove_file(&input_path);
    let _ = std::fs::remove_file(&output_path);
}
