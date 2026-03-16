//! Integration tests for the MKV demuxer.
//!
//! Validates that MkvDemuxer correctly parses Matroska files produced by MkvMuxer,
//! including MKV-specific codecs (H.264, H.265, AAC) that WebM does not support.

use std::io::Cursor;

use splica_core::{
    AudioCodec, AudioTrackInfo, Codec, Demuxer, Muxer, Packet, Timestamp, TrackIndex, TrackInfo,
    TrackKind, VideoCodec, VideoTrackInfo,
};
use splica_mkv::{MkvDemuxer, MkvMuxer};
use splica_webm::WebmMuxer;

fn make_h264_track(index: u32) -> TrackInfo {
    TrackInfo {
        index: TrackIndex(index),
        kind: TrackKind::Video,
        codec: Codec::Video(VideoCodec::H264),
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

fn make_aac_track(index: u32) -> TrackInfo {
    TrackInfo {
        index: TrackIndex(index),
        kind: TrackKind::Audio,
        codec: Codec::Audio(AudioCodec::Aac),
        duration: None,
        video: None,
        audio: Some(AudioTrackInfo {
            sample_rate: 44100,
            channel_layout: None,
            sample_format: None,
        }),
    }
}

fn make_packet(track: u32, frame_num: i64, is_keyframe: bool, data: &[u8]) -> Packet {
    Packet {
        track_index: TrackIndex(track),
        pts: Timestamp::new(frame_num * 33, 1000).unwrap(),
        dts: Timestamp::new(frame_num * 33, 1000).unwrap(),
        is_keyframe,
        data: bytes::Bytes::from(data.to_vec()),
    }
}

fn mux_to_webm_bytes(tracks: &[TrackInfo], packets: &[Packet]) -> Vec<u8> {
    let vp9_tracks: Vec<TrackInfo> = tracks
        .iter()
        .map(|t| {
            let mut t = t.clone();
            if t.kind == TrackKind::Video {
                t.codec = Codec::Video(VideoCodec::Other("VP9".to_string()));
            }
            if t.kind == TrackKind::Audio {
                t.codec = Codec::Audio(AudioCodec::Opus);
            }
            t
        })
        .collect();
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = WebmMuxer::new(&mut output);
        for track in &vp9_tracks {
            muxer.add_track(track).unwrap();
        }
        for pkt in packets {
            muxer.write_packet(pkt).unwrap();
        }
        muxer.finalize().unwrap();
    }
    output.into_inner()
}

fn mux_to_bytes(tracks: &[TrackInfo], packets: &[Packet]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut muxer = MkvMuxer::new(&mut output);
        for track in tracks {
            muxer.add_track(track).unwrap();
        }
        for pkt in packets {
            muxer.write_packet(pkt).unwrap();
        }
        muxer.finalize().unwrap();
    }
    output.into_inner()
}

#[test]
fn test_that_mkv_demuxer_parses_h264_and_aac_tracks() {
    // GIVEN — an MKV file with H.264 video + AAC audio
    let data = mux_to_bytes(
        &[make_h264_track(0), make_aac_track(1)],
        &[
            make_packet(0, 0, true, &[0x01, 0x02]),
            make_packet(1, 0, true, &[0x03, 0x04]),
            make_packet(0, 1, false, &[0x05, 0x06]),
        ],
    );

    // WHEN
    let demuxer = MkvDemuxer::open(Cursor::new(data)).unwrap();

    // THEN
    assert_eq!(demuxer.tracks().len(), 2);
    assert_eq!(demuxer.tracks()[0].codec, Codec::Video(VideoCodec::H264));
    assert_eq!(demuxer.tracks()[1].codec, Codec::Audio(AudioCodec::Aac));
}

#[test]
fn test_that_mkv_demuxer_reads_all_packets() {
    // GIVEN
    let data = mux_to_bytes(
        &[make_h264_track(0)],
        &[
            make_packet(0, 0, true, &[0xAA]),
            make_packet(0, 1, false, &[0xBB]),
            make_packet(0, 2, false, &[0xCC]),
        ],
    );

    // WHEN
    let mut demuxer = MkvDemuxer::open(Cursor::new(data)).unwrap();
    let mut packets = Vec::new();
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        packets.push(pkt);
    }

    // THEN
    assert_eq!(packets.len(), 3);
}

#[test]
fn test_that_mkv_demuxer_preserves_packet_data() {
    // GIVEN
    let data = mux_to_bytes(
        &[make_h264_track(0)],
        &[make_packet(0, 0, true, &[0xDE, 0xAD, 0xBE, 0xEF])],
    );

    // WHEN
    let mut demuxer = MkvDemuxer::open(Cursor::new(data)).unwrap();
    let pkt = demuxer.read_packet().unwrap().unwrap();

    // THEN
    assert_eq!(&pkt.data[..], &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn test_that_mkv_roundtrip_preserves_track_metadata() {
    // GIVEN — mux with H.264 + AAC, demux back
    let h264_track = make_h264_track(0);
    let aac_track = make_aac_track(1);
    let data = mux_to_bytes(
        &[h264_track, aac_track],
        &[
            make_packet(0, 0, true, &[0x01]),
            make_packet(1, 0, true, &[0x02]),
        ],
    );

    // WHEN
    let demuxer = MkvDemuxer::open(Cursor::new(data)).unwrap();

    // THEN — video track metadata
    let video = &demuxer.tracks()[0];
    assert_eq!(video.kind, TrackKind::Video);
    assert_eq!(video.codec, Codec::Video(VideoCodec::H264));
    let dims = video.video.as_ref().unwrap();
    assert_eq!(dims.width, 1920);
    assert_eq!(dims.height, 1080);

    // THEN — audio track metadata
    let audio = &demuxer.tracks()[1];
    assert_eq!(audio.kind, TrackKind::Audio);
    assert_eq!(audio.codec, Codec::Audio(AudioCodec::Aac));
}

#[test]
fn test_that_mkv_demuxer_preserves_keyframe_flags() {
    // GIVEN
    let data = mux_to_bytes(
        &[make_h264_track(0)],
        &[
            make_packet(0, 0, true, &[0x01]),
            make_packet(0, 1, false, &[0x02]),
            make_packet(0, 2, true, &[0x03]),
        ],
    );

    // WHEN
    let mut demuxer = MkvDemuxer::open(Cursor::new(data)).unwrap();

    // THEN
    assert!(demuxer.read_packet().unwrap().unwrap().is_keyframe);
    assert!(!demuxer.read_packet().unwrap().unwrap().is_keyframe);
    assert!(demuxer.read_packet().unwrap().unwrap().is_keyframe);
}

#[test]
fn test_that_mkv_demuxer_reads_h265_codec() {
    // GIVEN — MKV with H.265 video
    let track = TrackInfo {
        index: TrackIndex(0),
        kind: TrackKind::Video,
        codec: Codec::Video(VideoCodec::H265),
        duration: None,
        video: Some(VideoTrackInfo {
            width: 3840,
            height: 2160,
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
    };
    let data = mux_to_bytes(&[track], &[make_packet(0, 0, true, &[0x01])]);

    // WHEN
    let demuxer = MkvDemuxer::open(Cursor::new(data)).unwrap();

    // THEN
    assert_eq!(demuxer.tracks()[0].codec, Codec::Video(VideoCodec::H265));
}

#[test]
fn test_that_mkv_file_has_matroska_doctype() {
    // GIVEN — an MKV-muxed file
    let data = mux_to_bytes(&[make_h264_track(0)], &[make_packet(0, 0, true, &[0x01])]);

    // WHEN — we look for the "matroska" DocType string
    let matroska_bytes = b"matroska";
    let found = data
        .windows(matroska_bytes.len())
        .any(|w| w == matroska_bytes);

    // THEN
    assert!(found, "MKV output should contain 'matroska' DocType");
}

#[test]
fn test_that_webm_file_has_webm_doctype() {
    // GIVEN — a WebM-muxed file
    let vp9_track = TrackInfo {
        index: TrackIndex(0),
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
    };
    let data = mux_to_webm_bytes(&[vp9_track], &[make_packet(0, 0, true, &[0x01])]);

    // WHEN — we look for the "webm" DocType string (but not "matroska")
    let webm_bytes = b"webm";
    let matroska_bytes = b"matroska";
    let has_webm = data.windows(webm_bytes.len()).any(|w| w == webm_bytes);
    let has_matroska = data
        .windows(matroska_bytes.len())
        .any(|w| w == matroska_bytes);

    // THEN
    assert!(has_webm, "WebM output should contain 'webm' DocType");
    assert!(
        !has_matroska,
        "WebM output should not contain 'matroska' DocType"
    );
}
