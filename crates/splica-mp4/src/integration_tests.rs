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
