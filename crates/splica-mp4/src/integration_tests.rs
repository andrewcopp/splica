//! Integration tests for the MP4 demuxer.

use std::io::Cursor;

use splica_core::{AudioCodec, Codec, Demuxer, Muxer, ResourceBudget, TrackKind, VideoCodec};

use crate::demuxer::Mp4Demuxer;
use crate::error::Mp4Error;
use crate::test_helpers::{self, TestTrack};

#[test]
fn test_that_demuxer_extracts_track_count_and_codecs() {
    // GIVEN — a synthetic MP4 with 1 H.264 video track and 1 AAC audio track
    let mp4 = test_helpers::build_test_mp4(&[
        TestTrack::video(1920, 1080, 10),
        TestTrack::audio(44100, 20),
    ]);

    // WHEN
    let demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();
    let tracks = demuxer.tracks();

    // THEN
    assert_eq!(tracks.len(), 2);

    assert_eq!(tracks[0].kind, TrackKind::Video);
    assert_eq!(tracks[0].codec, Codec::Video(VideoCodec::H264));

    assert_eq!(tracks[1].kind, TrackKind::Audio);
    assert_eq!(tracks[1].codec, Codec::Audio(AudioCodec::Aac));
}

#[test]
fn test_that_demuxer_extracts_video_dimensions() {
    // GIVEN
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(1280, 720, 5)]);

    // WHEN
    let demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();
    let video_info = demuxer.tracks()[0].video.as_ref().unwrap();

    // THEN
    assert_eq!(video_info.width, 1280);
    assert_eq!(video_info.height, 720);
}

#[test]
fn test_that_demuxer_extracts_audio_sample_rate() {
    // GIVEN
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::audio(48000, 10)]);

    // WHEN
    let demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();
    let audio_info = demuxer.tracks()[0].audio.as_ref().unwrap();

    // THEN
    assert_eq!(audio_info.sample_rate, 48000);
}

#[test]
fn test_that_demuxer_extracts_duration() {
    // GIVEN — 30 samples at 30000/1001 fps (about 1 second)
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 30)]);

    // WHEN
    let demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();
    let duration = demuxer.tracks()[0].duration.unwrap();

    // THEN — 30 samples * 1001 delta / 30000 timescale ≈ 1.001 seconds
    let seconds = duration.as_seconds_f64();
    assert!((seconds - 1.001).abs() < 0.01);
}

#[test]
fn test_that_demuxer_reads_packets() {
    // GIVEN — 3 video samples
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 3)]);

    // WHEN
    let mut demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();
    let mut packets = Vec::new();
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        packets.push(pkt);
    }

    // THEN
    assert_eq!(packets.len(), 3);
    assert_eq!(packets[0].data.len(), 1000); // sample size from TestTrack::video
    assert!(packets[0].is_keyframe); // sample 1 is in stss
    assert!(!packets[1].is_keyframe);
}

#[test]
fn test_that_demuxer_rejects_empty_file() {
    // GIVEN — empty data
    let result = Mp4Demuxer::open(Cursor::new(Vec::new()));

    // THEN
    assert!(matches!(result, Err(Mp4Error::NotMp4)));
}

#[test]
fn test_that_demuxer_rejects_truncated_file() {
    // GIVEN — just a few bytes (not a valid MP4)
    let result = Mp4Demuxer::open(Cursor::new(vec![0u8; 4]));

    // THEN — should fail gracefully, not panic
    assert!(result.is_err());
}

#[test]
fn test_that_demuxer_rejects_non_mp4_file() {
    // GIVEN — a file that starts with something other than ftyp
    let mut data = vec![0u8; 32];
    // Write a box header that's not ftyp
    data[0..4].copy_from_slice(&32u32.to_be_bytes());
    data[4..8].copy_from_slice(b"RIFF"); // WAV header, not MP4

    let result = Mp4Demuxer::open(Cursor::new(data));

    // THEN
    assert!(matches!(result, Err(Mp4Error::NotMp4)));
}

#[test]
fn test_that_demuxer_handles_file_without_moov() {
    // GIVEN — valid ftyp but no moov
    let mut data = Vec::new();
    // ftyp box: 8 header + 8 body = 16 bytes
    let ftyp_size = 16u32;
    data.extend_from_slice(&ftyp_size.to_be_bytes());
    data.extend_from_slice(b"ftyp");
    data.extend_from_slice(b"isom"); // major_brand
    data.extend_from_slice(&0u32.to_be_bytes()); // minor_version

    let result = Mp4Demuxer::open(Cursor::new(data));

    // THEN
    assert!(matches!(result, Err(Mp4Error::MissingBox { name: "moov" })));
}

#[test]
fn test_that_demuxer_rejects_moov_exceeding_byte_budget() {
    // GIVEN — a valid MP4, but budget is too small for the moov box
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 3)]);
    let budget = ResourceBudget::new(16); // 16 bytes — far too small for any moov

    // WHEN
    let result = Mp4Demuxer::open_with_budget(Cursor::new(mp4), Some(budget));

    // THEN
    assert!(matches!(result, Err(Mp4Error::ResourceExhausted { .. })));
}

#[test]
fn test_that_demuxer_enforces_max_bytes_during_read() {
    // GIVEN — 3 video samples of 1000 bytes each
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 3)]);
    // Budget allows moov parsing but only ~1500 bytes of sample data (1 full + partial)
    let budget = ResourceBudget::new(1500);

    // WHEN
    let mut demuxer = Mp4Demuxer::open_with_budget(Cursor::new(mp4), Some(budget)).unwrap();
    let mut packets = Vec::new();
    loop {
        match demuxer.read_packet() {
            Ok(Some(pkt)) => packets.push(pkt),
            Ok(None) => break,
            Err(_) => break,
        }
    }

    // THEN — should read 1 packet (1000 bytes) but fail on the 2nd (would be 2000 > 1500)
    assert_eq!(packets.len(), 1);
}

#[test]
fn test_that_demuxer_enforces_max_frames_during_read() {
    // GIVEN — 5 video samples
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 5)]);
    let budget = ResourceBudget::new(u64::MAX).with_max_frames(2);

    // WHEN
    let mut demuxer = Mp4Demuxer::open_with_budget(Cursor::new(mp4), Some(budget)).unwrap();
    let mut packets = Vec::new();
    loop {
        match demuxer.read_packet() {
            Ok(Some(pkt)) => packets.push(pkt),
            Ok(None) => break,
            Err(_) => break,
        }
    }

    // THEN — should read exactly 2 packets
    assert_eq!(packets.len(), 2);
}

#[test]
fn test_that_muxer_produces_valid_mp4_from_demuxed_packets() {
    // GIVEN — demux a synthetic MP4 with 1 video + 1 audio track
    let original =
        test_helpers::build_test_mp4(&[TestTrack::video(640, 480, 5), TestTrack::audio(44100, 10)]);

    let mut demuxer = Mp4Demuxer::open(Cursor::new(original)).unwrap();
    let original_tracks = demuxer.tracks().to_vec();

    // WHEN — mux all packets into a new MP4
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = crate::muxer::Mp4Muxer::new(&mut output);
        for track in &original_tracks {
            muxer.add_track(track).unwrap();
        }
        while let Some(packet) = demuxer.read_packet().unwrap() {
            muxer.write_packet(&packet).unwrap();
        }
        muxer.finalize().unwrap();
    }

    // THEN — re-demux the output and verify track count and packet count
    let output_data = output.into_inner();

    let mut re_demuxer = Mp4Demuxer::open(Cursor::new(output_data)).unwrap();

    assert_eq!(re_demuxer.tracks().len(), 2);
    assert_eq!(re_demuxer.tracks()[0].kind, TrackKind::Video);
    assert_eq!(re_demuxer.tracks()[1].kind, TrackKind::Audio);

    // Count packets
    let mut packet_count = 0;
    while re_demuxer.read_packet().unwrap().is_some() {
        packet_count += 1;
    }
    assert_eq!(packet_count, 15); // 5 video + 10 audio
}

#[test]
fn test_that_muxer_enforces_byte_budget() {
    // GIVEN — demux a synthetic MP4 with 3 video samples
    let original = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 3)]);
    let mut demuxer = Mp4Demuxer::open(Cursor::new(original)).unwrap();
    let tracks = demuxer.tracks().to_vec();

    // Collect all packets
    let mut packets = Vec::new();
    while let Some(packet) = demuxer.read_packet().unwrap() {
        packets.push(packet);
    }
    assert_eq!(packets.len(), 3);

    // WHEN — mux with a byte budget that allows only 1 packet
    let first_packet_size = packets[0].data.len() as u64;
    let budget = ResourceBudget::new(first_packet_size + 1); // just enough for 1 packet

    let mut output = Cursor::new(Vec::new());
    let mut muxer = crate::muxer::Mp4Muxer::new_with_budget(&mut output, budget);
    for track in &tracks {
        muxer.add_track(track).unwrap();
    }

    // First packet should succeed
    muxer.write_packet(&packets[0]).unwrap();

    // THEN — second packet should fail with ResourceExhausted
    let result = muxer.write_packet(&packets[1]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), splica_core::ErrorKind::ResourceExhausted);
}

#[test]
fn test_that_muxer_enforces_frame_budget() {
    // GIVEN — demux a synthetic MP4 with 5 video samples
    let original = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 5)]);
    let mut demuxer = Mp4Demuxer::open(Cursor::new(original)).unwrap();
    let tracks = demuxer.tracks().to_vec();

    let mut packets = Vec::new();
    while let Some(packet) = demuxer.read_packet().unwrap() {
        packets.push(packet);
    }
    assert_eq!(packets.len(), 5);

    // WHEN — mux with a frame budget of 2
    let budget = ResourceBudget::new(u64::MAX).with_max_frames(2);

    let mut output = Cursor::new(Vec::new());
    let mut muxer = crate::muxer::Mp4Muxer::new_with_budget(&mut output, budget);
    for track in &tracks {
        muxer.add_track(track).unwrap();
    }

    // First two packets should succeed
    muxer.write_packet(&packets[0]).unwrap();
    muxer.write_packet(&packets[1]).unwrap();

    // THEN — third packet should fail with ResourceExhausted
    let result = muxer.write_packet(&packets[2]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), splica_core::ErrorKind::ResourceExhausted);
}

#[test]
fn test_that_metadata_round_trips_through_demux_mux() {
    // GIVEN — an MP4 with a udta box containing custom data
    let custom_data = b"splica-test-metadata-payload-12345";
    let udta = test_helpers::build_udta(custom_data);
    let base_mp4 = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 3)]);
    let mp4_with_metadata = test_helpers::inject_moov_boxes(&base_mp4, &[&udta]);

    // WHEN — demux, read metadata, remux with metadata
    let mut demuxer = Mp4Demuxer::open(Cursor::new(mp4_with_metadata)).unwrap();
    let metadata = demuxer.metadata().to_vec();

    assert_eq!(metadata.len(), 1);
    assert_eq!(metadata[0].box_type, crate::boxes::FourCC::UDTA);

    let tracks = demuxer.tracks().to_vec();
    let mut packets = Vec::new();
    while let Some(packet) = demuxer.read_packet().unwrap() {
        packets.push(packet);
    }

    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = crate::muxer::Mp4Muxer::new(&mut output);
        muxer.set_metadata(metadata);
        for track in &tracks {
            muxer.add_track(track).unwrap();
        }
        for packet in &packets {
            muxer.write_packet(packet).unwrap();
        }
        muxer.finalize().unwrap();
    }

    // THEN — re-demux and verify metadata is preserved
    let output_data = output.into_inner();
    let re_demuxer = Mp4Demuxer::open(Cursor::new(output_data)).unwrap();
    let re_metadata = re_demuxer.metadata();

    assert_eq!(re_metadata.len(), 1);
    assert_eq!(re_metadata[0].box_type, crate::boxes::FourCC::UDTA);
    // Verify the payload is preserved
    assert!(
        re_metadata[0]
            .data
            .windows(custom_data.len())
            .any(|w| w == custom_data),
        "custom metadata payload should be preserved in round-trip"
    );
}

#[test]
fn test_that_multiple_metadata_boxes_are_preserved() {
    // GIVEN — an MP4 with both udta and meta boxes
    let udta = test_helpers::build_udta(b"udta-payload");
    let meta = test_helpers::build_meta(b"meta-payload");
    let base_mp4 = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 2)]);
    let mp4_with_metadata = test_helpers::inject_moov_boxes(&base_mp4, &[&udta, &meta]);

    // WHEN
    let demuxer = Mp4Demuxer::open(Cursor::new(mp4_with_metadata)).unwrap();
    let metadata = demuxer.metadata();

    // THEN
    assert_eq!(metadata.len(), 2);
    assert_eq!(metadata[0].box_type, crate::boxes::FourCC::UDTA);
    assert_eq!(metadata[1].box_type, crate::boxes::FourCC::META);
}

#[test]
fn test_that_fmp4_muxer_writes_ftyp_moov_moof_mdat_structure() {
    // GIVEN — demux a synthetic MP4 with 1 video track (5 samples)
    let original = test_helpers::build_test_mp4(&[TestTrack::video(640, 480, 5)]);
    let mut demuxer = Mp4Demuxer::open(Cursor::new(original)).unwrap();
    let original_tracks = demuxer.tracks().to_vec();

    let mut packets = Vec::new();
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        packets.push(pkt);
    }

    // WHEN — mux into fragmented MP4
    let mut output = Vec::new();
    {
        let config = crate::fmp4_muxer::FragmentConfig {
            max_samples_per_fragment: 2,
        };
        let mut muxer = crate::fmp4_muxer::FragmentedMp4Muxer::with_config(&mut output, config);
        for track in &original_tracks {
            muxer.add_track(track).unwrap();
        }
        for packet in &packets {
            muxer.write_packet(packet).unwrap();
        }
        muxer.finalize().unwrap();
    }

    // THEN — verify the box structure: ftyp, moov (with mvex), then moof+mdat pairs
    let mut box_types = Vec::new();
    let mut pos = 0usize;
    while pos + 8 <= output.len() {
        let size = u32::from_be_bytes([
            output[pos],
            output[pos + 1],
            output[pos + 2],
            output[pos + 3],
        ]) as usize;
        let fourcc = [
            output[pos + 4],
            output[pos + 5],
            output[pos + 6],
            output[pos + 7],
        ];
        if size < 8 || pos + size > output.len() {
            break;
        }
        box_types.push(fourcc);
        pos += size;
    }

    assert_eq!(&box_types[0], b"ftyp");
    assert_eq!(&box_types[1], b"moov");

    // After ftyp+moov, expect alternating moof+mdat pairs
    let fragment_boxes = &box_types[2..];
    assert!(
        fragment_boxes.len() >= 2,
        "expected at least one moof+mdat pair"
    );
    assert_eq!(
        fragment_boxes.len() % 2,
        0,
        "moof+mdat should come in pairs"
    );
    for chunk in fragment_boxes.chunks(2) {
        assert_eq!(&chunk[0], b"moof");
        assert_eq!(&chunk[1], b"mdat");
    }
}

#[test]
fn test_that_fmp4_moov_contains_mvex() {
    // GIVEN — a minimal fragmented MP4
    let original = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 1)]);
    let mut demuxer = Mp4Demuxer::open(Cursor::new(original)).unwrap();
    let tracks = demuxer.tracks().to_vec();
    let mut packets = Vec::new();
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        packets.push(pkt);
    }

    let mut output = Vec::new();
    {
        let mut muxer = crate::fmp4_muxer::FragmentedMp4Muxer::new(&mut output);
        for track in &tracks {
            muxer.add_track(track).unwrap();
        }
        for packet in &packets {
            muxer.write_packet(packet).unwrap();
        }
        muxer.finalize().unwrap();
    }

    // WHEN — find the moov box and look for mvex inside it
    let moov = find_top_level_box(&output, b"moov").expect("moov not found");
    let has_mvex = find_child_box(&moov, b"mvex");

    // THEN
    assert!(has_mvex, "moov should contain mvex box for fragmented MP4");
}

#[test]
fn test_that_fmp4_muxer_only_requires_write() {
    // GIVEN — a Write-only sink (no Seek)
    struct WriteOnly(Vec<u8>);
    impl std::io::Write for WriteOnly {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let original = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 3)]);
    let mut demuxer = Mp4Demuxer::open(Cursor::new(original)).unwrap();
    let tracks = demuxer.tracks().to_vec();
    let mut packets = Vec::new();
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        packets.push(pkt);
    }

    // WHEN — mux to a Write-only target (no Seek)
    let mut output = WriteOnly(Vec::new());
    {
        let mut muxer = crate::fmp4_muxer::FragmentedMp4Muxer::new(&mut output);
        for track in &tracks {
            muxer.add_track(track).unwrap();
        }
        for packet in &packets {
            muxer.write_packet(packet).unwrap();
        }
        muxer.finalize().unwrap();
    }

    // THEN — output should be non-empty and start with ftyp
    assert!(output.0.len() > 8);
    assert_eq!(&output.0[4..8], b"ftyp");
}

#[test]
fn test_that_fmp4_sample_counts_match() {
    // GIVEN — 7 samples across 2 tracks
    let original =
        test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 4), TestTrack::audio(44100, 3)]);
    let mut demuxer = Mp4Demuxer::open(Cursor::new(original)).unwrap();
    let tracks = demuxer.tracks().to_vec();
    let mut packets = Vec::new();
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        packets.push(pkt);
    }
    let total_packets = packets.len();

    // WHEN — mux as fMP4
    let mut output = Vec::new();
    {
        let config = crate::fmp4_muxer::FragmentConfig {
            max_samples_per_fragment: 3,
        };
        let mut muxer = crate::fmp4_muxer::FragmentedMp4Muxer::with_config(&mut output, config);
        for track in &tracks {
            muxer.add_track(track).unwrap();
        }
        for packet in &packets {
            muxer.write_packet(packet).unwrap();
        }
        muxer.finalize().unwrap();
    }

    // THEN — count total sample bytes in all mdat boxes, should match input
    let total_input_bytes: usize = packets.iter().map(|p| p.data.len()).sum();
    let total_mdat_bytes = sum_mdat_body_bytes(&output);
    assert_eq!(
        total_mdat_bytes, total_input_bytes,
        "total sample data in mdat should match input packets"
    );

    // Also verify we wrote the expected number of packets
    assert_eq!(total_packets, 7);
}

// ---------------------------------------------------------------------------
// Seek tests
// ---------------------------------------------------------------------------

#[test]
fn test_that_seek_to_keyframe_resets_read_position() {
    // GIVEN — 5 video samples, first is keyframe
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 5)]);
    let mut demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();

    // Read all packets
    let mut count = 0;
    while demuxer.read_packet().unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 5);

    // WHEN — seek back to start
    use splica_core::Seekable;
    demuxer
        .seek(
            splica_core::Timestamp::new(0, 30000).unwrap(),
            splica_core::SeekMode::Keyframe,
        )
        .unwrap();

    // THEN — can read packets again
    let first = demuxer.read_packet().unwrap().unwrap();
    assert!(first.is_keyframe);
}

#[test]
fn test_that_seek_precise_finds_correct_sample() {
    // GIVEN — 10 video samples at 30000/1001 fps
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(320, 240, 10)]);
    let mut demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();

    // WHEN — seek to timestamp of ~3rd sample (3 * 1001 ticks at timescale 30000)
    use splica_core::Seekable;
    demuxer
        .seek(
            splica_core::Timestamp::new(3003, 30000).unwrap(),
            splica_core::SeekMode::Precise,
        )
        .unwrap();

    // THEN — next packet is at or after the seek point
    let pkt = demuxer.read_packet().unwrap().unwrap();
    assert!(pkt.pts.as_seconds_f64() >= 0.09); // ~0.1 seconds
}

#[test]
fn test_that_seek_on_empty_file_returns_error() {
    // GIVEN — MP4 with 0 samples (we need a valid MP4 with an empty track)
    // Use a 1-sample MP4 and read past it, then seek should still work on it.
    // Instead, test seeking on a file with no video track and audio only.
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::audio(44100, 0)]);

    let mut demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();

    // WHEN — seek on file with empty sample table
    use splica_core::Seekable;
    let result = demuxer.seek(
        splica_core::Timestamp::new(0, 1).unwrap(),
        splica_core::SeekMode::Keyframe,
    );

    // THEN — should error, not silently succeed
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Test helpers for fMP4 structure verification
// ---------------------------------------------------------------------------

/// Find a top-level box body by fourcc.
fn find_top_level_box(data: &[u8], target: &[u8; 4]) -> Option<Vec<u8>> {
    let mut pos = 0usize;
    while pos + 8 <= data.len() {
        let size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let fourcc = &data[pos + 4..pos + 8];
        if size < 8 || pos + size > data.len() {
            break;
        }
        if fourcc == target {
            return Some(data[pos + 8..pos + size].to_vec());
        }
        pos += size;
    }
    None
}

/// Check if a box body contains a child box with the given fourcc.
fn find_child_box(body: &[u8], target: &[u8; 4]) -> bool {
    let mut pos = 0usize;
    while pos + 8 <= body.len() {
        let size =
            u32::from_be_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]) as usize;
        let fourcc = &body[pos + 4..pos + 8];
        if size < 8 || pos + size > body.len() {
            break;
        }
        if fourcc == target {
            return true;
        }
        pos += size;
    }
    false
}

#[test]
fn test_that_video_packets_have_monotonic_timestamps_and_first_is_keyframe() {
    // GIVEN — a synthetic MP4 with 5 video samples
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(640, 480, 5)]);

    // WHEN — read all video packets
    let mut demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();
    let mut packets = Vec::new();
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        if pkt.track_index == demuxer.tracks()[0].index {
            packets.push(pkt);
        }
    }

    // THEN — first packet is a keyframe, timestamps are monotonically non-decreasing
    assert!(!packets.is_empty());
    assert!(packets[0].is_keyframe, "first packet should be a keyframe");

    for window in packets.windows(2) {
        let prev_us = window[0].pts.as_seconds_f64() * 1_000_000.0;
        let curr_us = window[1].pts.as_seconds_f64() * 1_000_000.0;
        assert!(
            curr_us >= prev_us,
            "timestamps should be monotonic: {prev_us} -> {curr_us}"
        );
    }
}

#[test]
fn test_that_codec_config_returns_avcc_for_h264_track() {
    // GIVEN — a synthetic MP4 with H.264 video
    let mp4 = test_helpers::build_test_mp4(&[TestTrack::video(1920, 1080, 3)]);

    // WHEN
    let demuxer = Mp4Demuxer::open(Cursor::new(mp4)).unwrap();
    let video_track = &demuxer.tracks()[0];
    let config = demuxer.codec_config(video_track.index);

    // THEN — should be Avc1 with non-empty avcC
    match config {
        Some(crate::boxes::stsd::CodecConfig::Avc1 {
            avcc,
            width,
            height,
            ..
        }) => {
            assert!(!avcc.is_empty(), "avcC data should not be empty");
            assert_eq!(*width, 1920);
            assert_eq!(*height, 1080);
        }
        other => panic!("expected Avc1 config, got {other:?}"),
    }
}

/// Sum all mdat body bytes in the file.
fn sum_mdat_body_bytes(data: &[u8]) -> usize {
    let mut total = 0;
    let mut pos = 0usize;
    while pos + 8 <= data.len() {
        let size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let fourcc = &data[pos + 4..pos + 8];
        if size < 8 || pos + size > data.len() {
            break;
        }
        if fourcc == b"mdat" {
            total += size - 8; // body = size minus 8-byte header
        }
        pos += size;
    }
    total
}
