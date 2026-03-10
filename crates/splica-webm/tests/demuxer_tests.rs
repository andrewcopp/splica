//! Integration tests for the WebM demuxer using synthetic WebM data.

use std::io::Cursor;

use splica_core::{Demuxer, TrackKind};
use splica_webm::WebmDemuxer;

// ---------------------------------------------------------------------------
// Helpers to build EBML encoded data
// ---------------------------------------------------------------------------

/// Encodes a value as an EBML element ID (preserving marker bits).
fn encode_element_id(id: u32) -> Vec<u8> {
    if id <= 0xFF {
        vec![id as u8]
    } else if id <= 0xFFFF {
        vec![(id >> 8) as u8, id as u8]
    } else if id <= 0xFF_FFFF {
        vec![(id >> 16) as u8, (id >> 8) as u8, id as u8]
    } else {
        vec![
            (id >> 24) as u8,
            (id >> 16) as u8,
            (id >> 8) as u8,
            id as u8,
        ]
    }
}

/// Encodes a size as an EBML vint (with marker bit).
fn encode_data_size(size: u64) -> Vec<u8> {
    if size < 0x7F {
        vec![(size as u8) | 0x80]
    } else if size < 0x3FFF {
        let val = size | 0x4000;
        vec![(val >> 8) as u8, val as u8]
    } else if size < 0x1F_FFFF {
        let val = size | 0x20_0000;
        vec![(val >> 16) as u8, (val >> 8) as u8, val as u8]
    } else {
        let val = size | 0x10_000000;
        vec![
            (val >> 24) as u8,
            (val >> 16) as u8,
            (val >> 8) as u8,
            val as u8,
        ]
    }
}

/// Builds an EBML element: ID + size + body.
fn element(id: u32, body: &[u8]) -> Vec<u8> {
    let mut out = encode_element_id(id);
    out.extend_from_slice(&encode_data_size(body.len() as u64));
    out.extend_from_slice(body);
    out
}

/// Builds an EBML unsigned integer element.
fn uint_element(id: u32, value: u64) -> Vec<u8> {
    // Minimal encoding
    let bytes = if value == 0 {
        vec![0]
    } else if value <= 0xFF {
        vec![value as u8]
    } else if value <= 0xFFFF {
        vec![(value >> 8) as u8, value as u8]
    } else if value <= 0xFF_FFFF {
        vec![(value >> 16) as u8, (value >> 8) as u8, value as u8]
    } else {
        value.to_be_bytes().to_vec()
    };
    element(id, &bytes)
}

/// Builds an EBML string element.
fn string_element(id: u32, s: &str) -> Vec<u8> {
    element(id, s.as_bytes())
}

/// Builds an EBML float element (8-byte).
fn float_element(id: u32, value: f64) -> Vec<u8> {
    element(id, &value.to_be_bytes())
}

/// Builds a SimpleBlock with the given track number, relative timestamp, keyframe flag, and data.
fn simple_block(track_number: u64, relative_ts: i16, keyframe: bool, data: &[u8]) -> Vec<u8> {
    // Track number as a vint (for small numbers, 1 byte with marker)
    let tn_vint = encode_data_size(track_number);
    let flags: u8 = if keyframe { 0x80 } else { 0x00 };

    let mut body = Vec::new();
    body.extend_from_slice(&tn_vint);
    body.extend_from_slice(&relative_ts.to_be_bytes());
    body.push(flags);
    body.extend_from_slice(data);

    element(0xA3, &body) // SIMPLE_BLOCK
}

/// Builds a minimal valid WebM file with one VP9 video track.
fn build_minimal_webm(video_frames: &[(i16, bool, &[u8])]) -> Vec<u8> {
    // EBML Header
    let ebml_body = [
        uint_element(0x4286, 1),        // EBMLVersion
        string_element(0x4282, "webm"), // DocType
    ]
    .concat();
    let ebml_header = element(0x1A45DFA3, &ebml_body);

    // Track Entry: VP9 video, 1920x1080
    let video_settings = [
        uint_element(0xB0, 1920), // PixelWidth
        uint_element(0xBA, 1080), // PixelHeight
    ]
    .concat();

    let track_entry = [
        uint_element(0xD7, 1),          // TrackNumber
        uint_element(0x83, 1),          // TrackType (video)
        string_element(0x86, "V_VP9"),  // CodecID
        element(0xE0, &video_settings), // Video
    ]
    .concat();

    let tracks = element(0x1654AE6B, &element(0xAE, &track_entry));

    // Info: TimestampScale = 1_000_000 (1ms)
    let info = element(0x1549A966, &uint_element(0x2AD7B1, 1_000_000));

    // Cluster with SimpleBlocks
    let mut cluster_body = uint_element(0xE7, 0); // ClusterTimestamp = 0
    for (ts, kf, data) in video_frames {
        cluster_body.extend_from_slice(&simple_block(1, *ts, *kf, data));
    }
    let cluster = element(0x1F43B675, &cluster_body);

    // Segment wrapping Info + Tracks + Cluster
    let segment_body = [info, tracks, cluster].concat();
    let segment = element(0x18538067, &segment_body);

    [ebml_header, segment].concat()
}

/// Builds a WebM file with both VP9 video and Opus audio tracks.
fn build_av_webm() -> Vec<u8> {
    let ebml_body = [uint_element(0x4286, 1), string_element(0x4282, "webm")].concat();
    let ebml_header = element(0x1A45DFA3, &ebml_body);

    // Video track
    let video_settings = [uint_element(0xB0, 640), uint_element(0xBA, 480)].concat();
    let video_track = [
        uint_element(0xD7, 1),
        uint_element(0x83, 1),
        string_element(0x86, "V_VP9"),
        element(0xE0, &video_settings),
    ]
    .concat();

    // Audio track
    let audio_settings = [float_element(0xB5, 48000.0), uint_element(0x9F, 2)].concat();
    let audio_track = [
        uint_element(0xD7, 2),
        uint_element(0x83, 2),
        string_element(0x86, "A_OPUS"),
        element(0xE1, &audio_settings),
    ]
    .concat();

    let tracks = element(
        0x1654AE6B,
        &[element(0xAE, &video_track), element(0xAE, &audio_track)].concat(),
    );

    let info = element(0x1549A966, &uint_element(0x2AD7B1, 1_000_000));

    // Cluster with interleaved video and audio packets
    let cluster_body = [
        uint_element(0xE7, 0),
        simple_block(1, 0, true, &[0xDE, 0xAD]), // video keyframe at t=0
        simple_block(2, 0, true, &[0xBE, 0xEF]), // audio at t=0
        simple_block(1, 33, false, &[0xCA, 0xFE]), // video at t=33ms
        simple_block(2, 20, true, &[0xBA, 0xBE]), // audio at t=20ms
    ]
    .concat();
    let cluster = element(0x1F43B675, &cluster_body);

    let segment_body = [info, tracks, cluster].concat();
    let segment = element(0x18538067, &segment_body);

    [ebml_header, segment].concat()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_that_webm_demuxer_reads_single_video_track() {
    let data = build_minimal_webm(&[(0, true, &[0x01, 0x02, 0x03])]);

    let cursor = Cursor::new(data);
    let demuxer = WebmDemuxer::open(cursor).unwrap();

    assert_eq!(demuxer.tracks().len(), 1);
    assert_eq!(demuxer.tracks()[0].kind, TrackKind::Video);
}

#[test]
fn test_that_webm_demuxer_reads_video_dimensions() {
    let data = build_minimal_webm(&[(0, true, &[0x01])]);

    let cursor = Cursor::new(data);
    let demuxer = WebmDemuxer::open(cursor).unwrap();
    let video = demuxer.tracks()[0].video.as_ref().unwrap();

    assert_eq!(video.width, 1920);
    assert_eq!(video.height, 1080);
}

#[test]
fn test_that_webm_demuxer_yields_packets() {
    let data = build_minimal_webm(&[
        (0, true, &[0xAA]),
        (33, false, &[0xBB]),
        (66, false, &[0xCC]),
    ]);

    let cursor = Cursor::new(data);
    let mut demuxer = WebmDemuxer::open(cursor).unwrap();

    let pkt1 = demuxer.read_packet().unwrap().unwrap();
    assert!(pkt1.is_keyframe);
    assert_eq!(&pkt1.data[..], &[0xAA]);

    let pkt2 = demuxer.read_packet().unwrap().unwrap();
    assert!(!pkt2.is_keyframe);
    assert_eq!(&pkt2.data[..], &[0xBB]);

    let pkt3 = demuxer.read_packet().unwrap().unwrap();
    assert!(!pkt3.is_keyframe);
    assert_eq!(&pkt3.data[..], &[0xCC]);

    let end = demuxer.read_packet().unwrap();
    assert!(end.is_none());
}

#[test]
fn test_that_webm_demuxer_reads_two_tracks() {
    let data = build_av_webm();

    let cursor = Cursor::new(data);
    let demuxer = WebmDemuxer::open(cursor).unwrap();

    assert_eq!(demuxer.tracks().len(), 2);
    assert_eq!(demuxer.tracks()[0].kind, TrackKind::Video);
    assert_eq!(demuxer.tracks()[1].kind, TrackKind::Audio);
}

#[test]
fn test_that_webm_demuxer_yields_interleaved_packets() {
    let data = build_av_webm();

    let cursor = Cursor::new(data);
    let mut demuxer = WebmDemuxer::open(cursor).unwrap();

    let mut packets = Vec::new();
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        packets.push(pkt);
    }

    assert_eq!(packets.len(), 4);
}

#[test]
fn test_that_webm_demuxer_rejects_non_webm() {
    let data = vec![0x00, 0x01, 0x02, 0x03];
    let cursor = Cursor::new(data);
    let result = WebmDemuxer::open(cursor);

    assert!(result.is_err());
}

#[test]
fn test_that_webm_demuxer_reports_audio_settings() {
    let data = build_av_webm();

    let cursor = Cursor::new(data);
    let demuxer = WebmDemuxer::open(cursor).unwrap();
    let audio = demuxer.tracks()[1].audio.as_ref().unwrap();

    assert_eq!(audio.sample_rate, 48000);
}

/// A reader wrapper that tracks total bytes read via a shared counter,
/// proving the demuxer doesn't read the entire file during open().
struct TrackingReader<R> {
    inner: R,
    bytes_read: std::rc::Rc<std::cell::Cell<usize>>,
}

impl<R> TrackingReader<R> {
    fn new(inner: R) -> (Self, std::rc::Rc<std::cell::Cell<usize>>) {
        let counter = std::rc::Rc::new(std::cell::Cell::new(0));
        (
            Self {
                inner,
                bytes_read: counter.clone(),
            },
            counter,
        )
    }
}

impl<R: std::io::Read> std::io::Read for TrackingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.bytes_read.set(self.bytes_read.get() + n);
        Ok(n)
    }
}

impl<R: std::io::Seek> std::io::Seek for TrackingReader<R> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[test]
fn test_that_open_does_not_read_cluster_data() {
    // Build a WebM with enough frames that cluster data is significant
    let frames: Vec<(i16, bool, &[u8])> = (0..100)
        .map(|i| {
            let ts = i as i16;
            let kf = i == 0;
            (ts, kf, &[0xAA, 0xBB, 0xCC, 0xDD][..])
        })
        .collect();
    let data = build_minimal_webm(&frames);
    let total_file_size = data.len();

    let (tracking, counter) = TrackingReader::new(Cursor::new(data));
    let demuxer = WebmDemuxer::open(tracking).unwrap();

    // open() should have read far less than the full file
    // (only EBML header + Info + Tracks, not the cluster data)
    let bytes_read = counter.get();
    assert!(
        bytes_read < total_file_size,
        "open() read {bytes_read} bytes but file is {total_file_size} bytes — \
         should not read cluster data during open"
    );

    // Verify the demuxer still works correctly
    assert_eq!(demuxer.tracks().len(), 1);
}

#[test]
fn test_that_streaming_read_yields_all_packets() {
    let frames: Vec<(i16, bool, &[u8])> = (0..10)
        .map(|i| {
            let ts = i as i16;
            let kf = i == 0;
            (ts, kf, &[0xAA, 0xBB][..])
        })
        .collect();
    let data = build_minimal_webm(&frames);

    let cursor = Cursor::new(data);
    let mut demuxer = WebmDemuxer::open(cursor).unwrap();

    let mut count = 0;
    while demuxer.read_packet().unwrap().is_some() {
        count += 1;
    }

    assert_eq!(count, 10);
}
