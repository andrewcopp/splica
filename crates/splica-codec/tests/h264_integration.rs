//! Integration tests for H.264 decoder.
//!
//! Uses the openh264 encoder to generate real H.264 data, then decodes it
//! with our H264Decoder and verifies the output frames.

#![cfg(feature = "codec-h264")]

use bytes::Bytes;
use openh264::encoder::Encoder;
use openh264::formats::YUVSource;
use splica_codec::h264::avcc::AvcDecoderConfig;
use splica_codec::H264Decoder;
use splica_core::media::{Packet, PixelFormat, TrackIndex};
use splica_core::timestamp::Timestamp;
use splica_core::Decoder;

/// A simple YUV420p source for the encoder.
struct TestYuv {
    data: Vec<u8>,
    width: usize,
    height: usize,
}

impl TestYuv {
    fn green(width: usize, height: usize) -> Self {
        let y_size = width * height;
        let uv_size = (width / 2) * (height / 2);
        let mut data = vec![0u8; y_size + 2 * uv_size];
        // Green in YUV: Y≈149, U≈43, V≈21
        data[..y_size].fill(149);
        data[y_size..y_size + uv_size].fill(43);
        data[y_size + uv_size..].fill(21);
        Self {
            data,
            width,
            height,
        }
    }
}

impl YUVSource for TestYuv {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn y(&self) -> &[u8] {
        &self.data[..self.width * self.height]
    }

    fn u(&self) -> &[u8] {
        let y_size = self.width * self.height;
        let uv_size = (self.width / 2) * (self.height / 2);
        &self.data[y_size..y_size + uv_size]
    }

    fn v(&self) -> &[u8] {
        let y_size = self.width * self.height;
        let uv_size = (self.width / 2) * (self.height / 2);
        &self.data[y_size + uv_size..]
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.width, self.width / 2, self.width / 2)
    }
}

/// Encodes test frames and returns (avcC data, Vec of MP4 length-prefixed packets).
fn encode_test_frames(width: usize, height: usize, count: usize) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut encoder = Encoder::new().unwrap();

    let mut annex_b_packets = Vec::new();
    for _ in 0..count {
        let yuv = TestYuv::green(width, height);
        let bitstream = encoder.encode(&yuv).unwrap();
        annex_b_packets.push(bitstream.to_vec());
    }

    // Extract SPS/PPS from the Annex B bitstream
    let mut sps_list = Vec::new();
    let mut pps_list = Vec::new();
    for packet in &annex_b_packets {
        extract_sps_pps(packet, &mut sps_list, &mut pps_list);
        if !sps_list.is_empty() && !pps_list.is_empty() {
            break;
        }
    }

    let avcc = build_avcc(&sps_list, &pps_list);
    let mp4_packets: Vec<Vec<u8>> = annex_b_packets.iter().map(|p| annex_b_to_mp4(p)).collect();

    (avcc, mp4_packets)
}

fn extract_sps_pps(data: &[u8], sps: &mut Vec<Vec<u8>>, pps: &mut Vec<Vec<u8>>) {
    let nals = split_annex_b(data);
    for nal in nals {
        if nal.is_empty() {
            continue;
        }
        let nal_type = nal[0] & 0x1F;
        match nal_type {
            7 => sps.push(nal),
            8 => pps.push(nal),
            _ => {}
        }
    }
}

fn split_annex_b(data: &[u8]) -> Vec<Vec<u8>> {
    let mut nals = Vec::new();
    let mut i = 0;

    while i < data.len() {
        // Find start code
        if i + 3 < data.len()
            && data[i] == 0
            && data[i + 1] == 0
            && data[i + 2] == 0
            && data[i + 3] == 1
        {
            i += 4;
        } else if i + 2 < data.len() && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            i += 3;
        } else {
            i += 1;
            continue;
        }

        // Find next start code
        let nal_start = i;
        while i < data.len() {
            if i + 3 < data.len()
                && data[i] == 0
                && data[i + 1] == 0
                && data[i + 2] == 0
                && data[i + 3] == 1
            {
                break;
            }
            if i + 2 < data.len() && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
                break;
            }
            i += 1;
        }

        if nal_start < i {
            nals.push(data[nal_start..i].to_vec());
        }
    }

    nals
}

fn build_avcc(sps_list: &[Vec<u8>], pps_list: &[Vec<u8>]) -> Vec<u8> {
    let mut avcc = Vec::new();
    let profile = sps_list
        .first()
        .and_then(|s| s.get(1).copied())
        .unwrap_or(0x42);
    let compat = sps_list
        .first()
        .and_then(|s| s.get(2).copied())
        .unwrap_or(0xC0);
    let level = sps_list
        .first()
        .and_then(|s| s.get(3).copied())
        .unwrap_or(0x1E);

    avcc.push(1); // version
    avcc.push(profile);
    avcc.push(compat);
    avcc.push(level);
    avcc.push(0xFF); // nal_length_size = 4
    avcc.push(0xE0 | (sps_list.len() as u8 & 0x1F));
    for sps in sps_list {
        avcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(sps);
    }
    avcc.push(pps_list.len() as u8);
    for pps in pps_list {
        avcc.extend_from_slice(&(pps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(pps);
    }
    avcc
}

/// Converts Annex B to MP4 length-prefixed format (4-byte lengths).
fn annex_b_to_mp4(data: &[u8]) -> Vec<u8> {
    let nals = split_annex_b(data);
    let mut out = Vec::new();
    for nal in nals {
        let len = nal.len() as u32;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&nal);
    }
    out
}

#[test]
fn test_that_h264_decoder_decodes_encoded_frames() {
    // GIVEN — encode 3 green frames at 64x64
    let (avcc, mp4_packets) = encode_test_frames(64, 64, 3);

    // Verify avcC is parseable
    let config = AvcDecoderConfig::parse(&avcc).unwrap();
    assert!(!config.sps.is_empty(), "should have at least one SPS");
    assert!(!config.pps.is_empty(), "should have at least one PPS");

    // WHEN — decode all packets
    let mut decoder = H264Decoder::new(&avcc).unwrap();
    let mut frames = Vec::new();

    for (i, pkt_data) in mp4_packets.iter().enumerate() {
        let packet = Packet {
            track_index: TrackIndex(0),
            pts: Timestamp::new(i as i64, 30).unwrap(),
            dts: Timestamp::new(i as i64, 30).unwrap(),
            is_keyframe: i == 0,
            data: Bytes::from(pkt_data.clone()),
        };

        decoder.send_packet(Some(&packet)).unwrap();
        while let Some(frame) = decoder.receive_frame().unwrap() {
            frames.push(frame);
        }
    }

    // Flush
    decoder.send_packet(None).unwrap();
    while let Some(frame) = decoder.receive_frame().unwrap() {
        frames.push(frame);
    }

    // THEN — should have decoded at least 1 frame
    assert!(
        !frames.is_empty(),
        "decoder should produce at least one frame"
    );

    // Verify frame properties
    for frame in &frames {
        match frame {
            splica_core::media::Frame::Video(vf) => {
                assert_eq!(vf.width, 64);
                assert_eq!(vf.height, 64);
                assert_eq!(vf.pixel_format, PixelFormat::Yuv420p);
                assert_eq!(vf.planes.len(), 3);
                assert_eq!(vf.planes[0].width, 64);
                assert_eq!(vf.planes[0].height, 64);
                assert_eq!(vf.planes[1].width, 32);
                assert_eq!(vf.planes[1].height, 32);
                assert_eq!(vf.planes[2].width, 32);
                assert_eq!(vf.planes[2].height, 32);
            }
            _ => panic!("expected video frame"),
        }
    }
}

#[test]
fn test_that_h264_decoder_rejects_invalid_avcc() {
    // GIVEN — garbage avcC data
    let result = H264Decoder::new(&[0xFF, 0xFF]);

    // THEN — should fail gracefully
    assert!(result.is_err());
}
