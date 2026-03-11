//! Integration tests for H.265 encode → decode round-trip.
//!
//! Uses the kvazaar encoder to generate real H.265 data, then decodes it
//! with H265Decoder and verifies the output frames.

#![cfg(all(feature = "codec-h265", feature = "codec-h265-enc"))]

use bytes::Bytes;
use splica_codec::h265::hvcc::HevcDecoderConfig;
use splica_codec::{H265Decoder, H265EncoderBuilder};
use splica_core::media::{Frame, Packet, PixelFormat, TrackIndex, VideoFrame};
use splica_core::timestamp::Timestamp;
use splica_core::{Decoder, Encoder};

/// Creates a synthetic YUV420p green frame at the given dimensions.
fn make_test_frame(width: u32, height: u32, pts_tick: i64) -> Frame {
    let y_size = (width * height) as usize;
    let uv_size = ((width / 2) * (height / 2)) as usize;
    let mut data = vec![0u8; y_size + 2 * uv_size];
    // Green in YUV: Y≈149, U≈43, V≈21
    data[..y_size].fill(149);
    data[y_size..y_size + uv_size].fill(43);
    data[y_size + uv_size..].fill(21);

    let planes = vec![
        splica_core::media::PlaneLayout {
            offset: 0,
            stride: width as usize,
            width,
            height,
        },
        splica_core::media::PlaneLayout {
            offset: y_size,
            stride: (width / 2) as usize,
            width: width / 2,
            height: height / 2,
        },
        splica_core::media::PlaneLayout {
            offset: y_size + uv_size,
            stride: (width / 2) as usize,
            width: width / 2,
            height: height / 2,
        },
    ];

    let pts = Timestamp::new(pts_tick, 30).unwrap();
    let vf = VideoFrame::new(
        width,
        height,
        PixelFormat::Yuv420p,
        None,
        pts,
        Bytes::from(data),
        planes,
    )
    .unwrap();
    Frame::Video(vf)
}

/// Splits Annex B byte stream into individual NAL units.
fn split_annex_b(data: &[u8]) -> Vec<Vec<u8>> {
    let mut nals = Vec::new();
    let mut i = 0;

    while i < data.len() {
        // Find start code (00 00 00 01 or 00 00 01)
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

/// Returns the H.265 NAL unit type from the 2-byte NAL header.
fn hevc_nal_type(nal: &[u8]) -> u8 {
    if nal.is_empty() {
        return 0;
    }
    (nal[0] >> 1) & 0x3F
}

/// Extracted H.265 NAL units grouped by type.
struct HevcNalSets {
    vps: Vec<Vec<u8>>,
    sps: Vec<Vec<u8>>,
    pps: Vec<Vec<u8>>,
    slices: Vec<Vec<u8>>,
}

/// Extracts VPS/SPS/PPS from Annex B data and separates slice NALs.
fn extract_parameter_sets(annex_b: &[u8]) -> HevcNalSets {
    let nals = split_annex_b(annex_b);
    let mut vps = Vec::new();
    let mut sps = Vec::new();
    let mut pps = Vec::new();
    let mut slices = Vec::new();

    for nal in nals {
        match hevc_nal_type(&nal) {
            32 => vps.push(nal),
            33 => sps.push(nal),
            34 => pps.push(nal),
            _ => slices.push(nal),
        }
    }
    HevcNalSets {
        vps,
        sps,
        pps,
        slices,
    }
}

/// Builds a minimal hvcC record from VPS/SPS/PPS NAL units.
fn build_hvcc(vps: &[Vec<u8>], sps: &[Vec<u8>], pps: &[Vec<u8>]) -> Vec<u8> {
    let mut hvcc = vec![0u8; 23];

    hvcc[0] = 1; // configurationVersion
                 // general_profile_space(0) | general_tier_flag(0) | general_profile_idc(1 = Main)
    hvcc[1] = 0x01;
    // general_profile_compatibility_flags (bit 1 set for Main)
    hvcc[2] = 0x60;
    // general_constraint_indicator_flags (6 bytes) — zeros
    // general_level_idc (93 = level 3.1)
    hvcc[12] = 93;
    // min_spatial_segmentation_idc (reserved 4 bits = 0xF)
    hvcc[13] = 0xF0;
    hvcc[14] = 0x00;
    // parallelismType (reserved 6 bits = 0xFC)
    hvcc[15] = 0xFC;
    // chroma_format_idc (reserved 6 bits = 0xFC, chroma=1 for 4:2:0)
    hvcc[16] = 0xFD;
    // bit_depth_luma_minus8 (reserved 5 bits = 0xF8)
    hvcc[17] = 0xF8;
    // bit_depth_chroma_minus8 (reserved 5 bits = 0xF8)
    hvcc[18] = 0xF8;
    // avgFrameRate
    hvcc[19] = 0;
    hvcc[20] = 0;
    // constantFrameRate(0) | numTemporalLayers(1) | temporalIdNested(1) | lengthSizeMinusOne(3)
    hvcc[21] = 0x0F;
    // numOfArrays
    hvcc[22] = 3;

    // VPS array
    hvcc.push(0x20); // array_completeness(0) | reserved(0) | NAL_unit_type(32)
    hvcc.extend_from_slice(&(vps.len() as u16).to_be_bytes());
    for v in vps {
        hvcc.extend_from_slice(&(v.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(v);
    }

    // SPS array
    hvcc.push(0x21);
    hvcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    for s in sps {
        hvcc.extend_from_slice(&(s.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(s);
    }

    // PPS array
    hvcc.push(0x22);
    hvcc.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    for p in pps {
        hvcc.extend_from_slice(&(p.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(p);
    }

    hvcc
}

/// Converts slice NAL units to MP4 length-prefixed format (4-byte lengths).
fn nals_to_mp4(nals: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for nal in nals {
        let len = nal.len() as u32;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

#[test]
fn test_that_h265_decoder_decodes_encoded_frames() {
    // GIVEN — encode 30 green frames at 64x64 with kvazaar
    // (libde265 buffers frames due to B-frame reordering, needs enough data)
    let mut encoder = H265EncoderBuilder::new()
        .dimensions(64, 64)
        .build()
        .unwrap();

    let mut all_annex_b = Vec::new();
    let mut packet_boundaries = Vec::new();

    for i in 0..30 {
        let frame = make_test_frame(64, 64, i);
        encoder.send_frame(Some(&frame)).unwrap();
        while let Some(pkt) = encoder.receive_packet().unwrap() {
            let start = all_annex_b.len();
            all_annex_b.extend_from_slice(&pkt.data);
            packet_boundaries.push((start, all_annex_b.len()));
        }
    }

    // Flush encoder
    encoder.send_frame(None).unwrap();
    while let Some(pkt) = encoder.receive_packet().unwrap() {
        let start = all_annex_b.len();
        all_annex_b.extend_from_slice(&pkt.data);
        packet_boundaries.push((start, all_annex_b.len()));
    }

    assert!(
        !packet_boundaries.is_empty(),
        "encoder should produce at least one packet"
    );

    // Extract VPS/SPS/PPS from the first packet (encoder prepends them)
    let first_packet = &all_annex_b[packet_boundaries[0].0..packet_boundaries[0].1];
    let nal_sets = extract_parameter_sets(first_packet);
    let vps = &nal_sets.vps;
    let sps = &nal_sets.sps;
    let pps = &nal_sets.pps;
    assert!(!vps.is_empty(), "should find VPS in first packet");
    assert!(!sps.is_empty(), "should find SPS in first packet");
    assert!(!pps.is_empty(), "should find PPS in first packet");

    // Build hvcC for decoder initialization
    let hvcc = build_hvcc(vps, sps, pps);

    // Verify hvcC is parseable
    let config = HevcDecoderConfig::parse(&hvcc).unwrap();
    assert_eq!(config.nal_length_size, 4);
    assert!(!config.vps.is_empty());

    // WHEN — decode all packets
    let mut decoder = H265Decoder::new(&hvcc).unwrap();
    let mut frames = Vec::new();

    for (i, &(start, end)) in packet_boundaries.iter().enumerate() {
        let pkt_annex_b = &all_annex_b[start..end];
        let slice_nals = extract_parameter_sets(pkt_annex_b).slices;
        let mp4_data = nals_to_mp4(&slice_nals);

        if mp4_data.is_empty() {
            continue;
        }

        let packet = Packet {
            track_index: TrackIndex(0),
            pts: Timestamp::new(i as i64, 30).unwrap(),
            dts: Timestamp::new(i as i64, 30).unwrap(),
            is_keyframe: i == 0,
            data: Bytes::from(mp4_data),
        };

        decoder.send_packet(Some(&packet)).unwrap();
        while let Some(frame) = decoder.receive_frame().unwrap() {
            frames.push(frame);
        }
    }

    // Flush decoder
    decoder.send_packet(None).unwrap();
    while let Some(frame) = decoder.receive_frame().unwrap() {
        frames.push(frame);
    }

    // THEN — should have decoded at least 1 frame with correct dimensions
    assert!(
        !frames.is_empty(),
        "decoder should produce at least one frame (got {} packets from encoder)",
        packet_boundaries.len()
    );

    for frame in &frames {
        match frame {
            Frame::Video(vf) => {
                assert_eq!(vf.width, 64);
                assert_eq!(vf.height, 64);
                assert_eq!(vf.pixel_format, PixelFormat::Yuv420p);
                assert_eq!(vf.planes.len(), 3);
            }
            _ => panic!("expected video frame"),
        }
    }
}

#[test]
fn test_that_h265_decoder_rejects_invalid_hvcc() {
    // GIVEN — garbage hvcC data
    let result = H265Decoder::new(&[0xFF, 0xFF]);

    // THEN — should fail gracefully
    assert!(result.is_err());
}
