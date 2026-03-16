//! Integration tests for the WebM muxer.
//!
//! Validates that muxed output can be demuxed back correctly (round-trip).

use std::io::Cursor;

use splica_core::{
    AudioCodec, AudioTrackInfo, Codec, Demuxer, Muxer, Packet, Timestamp, TrackIndex, TrackInfo,
    TrackKind, VideoCodec, VideoTrackInfo,
};
use splica_webm::{WebmDemuxer, WebmMuxer};

fn make_video_track(index: u32) -> TrackInfo {
    TrackInfo {
        index: TrackIndex(index),
        kind: TrackKind::Video,
        codec: Codec::Video(VideoCodec::Other("VP9".to_string())),
        duration: None,
        video: Some(VideoTrackInfo {
            width: 1920,
            height: 1080,
            pixel_format: None,
            color_space: None,
            frame_rate: None,
            profile: None,
            level: None,
            color_primaries: None,
            transfer_characteristics: None,
            matrix_coefficients: None,
        }),
        audio: None,
    }
}

fn make_audio_track(index: u32) -> TrackInfo {
    TrackInfo {
        index: TrackIndex(index),
        kind: TrackKind::Audio,
        codec: Codec::Audio(AudioCodec::Opus),
        duration: None,
        video: None,
        audio: Some(AudioTrackInfo {
            sample_rate: 48000,
            channel_layout: None,
            sample_format: None,
        }),
    }
}

fn make_packet(track: u32, frame_num: i64, is_keyframe: bool, data: &[u8]) -> Packet {
    Packet {
        track_index: TrackIndex(track),
        pts: Timestamp::new(frame_num * 33, 1000).unwrap(), // ~30fps in ms timebase
        dts: Timestamp::new(frame_num * 33, 1000).unwrap(),
        is_keyframe,
        data: bytes::Bytes::from(data.to_vec()),
    }
}

#[test]
fn test_that_muxer_produces_demuxable_output() {
    // GIVEN
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = WebmMuxer::new(&mut output);
        muxer.add_track(&make_video_track(0)).unwrap();

        // Write 3 packets
        muxer
            .write_packet(&make_packet(0, 0, true, &[0xAA, 0xBB]))
            .unwrap();
        muxer
            .write_packet(&make_packet(0, 1, false, &[0xCC, 0xDD]))
            .unwrap();
        muxer
            .write_packet(&make_packet(0, 2, false, &[0xEE, 0xFF]))
            .unwrap();
        muxer.finalize().unwrap();
    }

    // WHEN — demux the output
    let data = output.into_inner();
    let mut demuxer = WebmDemuxer::open(Cursor::new(data)).unwrap();

    // THEN — should have 1 video track
    assert_eq!(demuxer.tracks().len(), 1);
    assert_eq!(demuxer.tracks()[0].kind, TrackKind::Video);

    // THEN — should yield 3 packets
    let mut packets = Vec::new();
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        packets.push(pkt);
    }
    assert_eq!(packets.len(), 3);
}

#[test]
fn test_that_muxer_preserves_keyframe_flags() {
    // GIVEN
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = WebmMuxer::new(&mut output);
        muxer.add_track(&make_video_track(0)).unwrap();

        muxer
            .write_packet(&make_packet(0, 0, true, &[0x01]))
            .unwrap();
        muxer
            .write_packet(&make_packet(0, 1, false, &[0x02]))
            .unwrap();
        muxer
            .write_packet(&make_packet(0, 2, true, &[0x03]))
            .unwrap();
        muxer.finalize().unwrap();
    }

    // WHEN
    let data = output.into_inner();
    let mut demuxer = WebmDemuxer::open(Cursor::new(data)).unwrap();

    // THEN
    let pkt1 = demuxer.read_packet().unwrap().unwrap();
    assert!(pkt1.is_keyframe);

    let pkt2 = demuxer.read_packet().unwrap().unwrap();
    assert!(!pkt2.is_keyframe);

    let pkt3 = demuxer.read_packet().unwrap().unwrap();
    assert!(pkt3.is_keyframe);
}

#[test]
fn test_that_muxer_preserves_packet_data() {
    // GIVEN
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = WebmMuxer::new(&mut output);
        muxer.add_track(&make_video_track(0)).unwrap();

        muxer
            .write_packet(&make_packet(0, 0, true, &[0xDE, 0xAD, 0xBE, 0xEF]))
            .unwrap();
        muxer.finalize().unwrap();
    }

    // WHEN
    let data = output.into_inner();
    let mut demuxer = WebmDemuxer::open(Cursor::new(data)).unwrap();

    // THEN
    let pkt = demuxer.read_packet().unwrap().unwrap();
    assert_eq!(&pkt.data[..], &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn test_that_muxer_handles_two_tracks() {
    // GIVEN
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = WebmMuxer::new(&mut output);
        muxer.add_track(&make_video_track(0)).unwrap();
        muxer.add_track(&make_audio_track(1)).unwrap();

        // Interleaved packets
        muxer
            .write_packet(&make_packet(0, 0, true, &[0x01]))
            .unwrap();
        muxer
            .write_packet(&make_packet(1, 0, true, &[0x02]))
            .unwrap();
        muxer
            .write_packet(&make_packet(0, 1, false, &[0x03]))
            .unwrap();
        muxer
            .write_packet(&make_packet(1, 1, true, &[0x04]))
            .unwrap();
        muxer.finalize().unwrap();
    }

    // WHEN
    let data = output.into_inner();
    let mut demuxer = WebmDemuxer::open(Cursor::new(data)).unwrap();

    // THEN — 2 tracks
    assert_eq!(demuxer.tracks().len(), 2);
    assert_eq!(demuxer.tracks()[0].kind, TrackKind::Video);
    assert_eq!(demuxer.tracks()[1].kind, TrackKind::Audio);

    // THEN — 4 packets total
    let mut count = 0;
    while demuxer.read_packet().unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 4);
}

#[test]
fn test_that_muxer_reads_video_dimensions() {
    // GIVEN
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = WebmMuxer::new(&mut output);
        muxer.add_track(&make_video_track(0)).unwrap();
        muxer
            .write_packet(&make_packet(0, 0, true, &[0x01]))
            .unwrap();
        muxer.finalize().unwrap();
    }

    // WHEN
    let data = output.into_inner();
    let demuxer = WebmDemuxer::open(Cursor::new(data)).unwrap();

    // THEN
    let video = demuxer.tracks()[0].video.as_ref().unwrap();
    assert_eq!(video.width, 1920);
    assert_eq!(video.height, 1080);
}

#[test]
fn test_that_muxer_reads_audio_sample_rate() {
    // GIVEN
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = WebmMuxer::new(&mut output);
        muxer.add_track(&make_audio_track(0)).unwrap();
        muxer
            .write_packet(&make_packet(0, 0, true, &[0x01]))
            .unwrap();
        muxer.finalize().unwrap();
    }

    // WHEN
    let data = output.into_inner();
    let demuxer = WebmDemuxer::open(Cursor::new(data)).unwrap();

    // THEN
    let audio = demuxer.tracks()[0].audio.as_ref().unwrap();
    assert_eq!(audio.sample_rate, 48000);
}

#[test]
fn test_that_muxer_rejects_adding_tracks_after_writing() {
    // GIVEN
    let mut output = Cursor::new(Vec::new());
    let mut muxer = WebmMuxer::new(&mut output);
    muxer.add_track(&make_video_track(0)).unwrap();
    muxer
        .write_packet(&make_packet(0, 0, true, &[0x01]))
        .unwrap();

    // WHEN
    let result = muxer.add_track(&make_video_track(1));

    // THEN
    assert!(result.is_err());
}

#[test]
fn test_that_muxer_rejects_unsupported_codec() {
    // GIVEN
    let mut output = Cursor::new(Vec::new());
    let mut muxer = WebmMuxer::new(&mut output);

    let track = TrackInfo {
        index: TrackIndex(0),
        kind: TrackKind::Video,
        codec: Codec::Video(VideoCodec::Other("ProRes".to_string())),
        duration: None,
        video: None,
        audio: None,
    };

    // WHEN
    let result = muxer.add_track(&track);

    // THEN
    assert!(result.is_err());
}
