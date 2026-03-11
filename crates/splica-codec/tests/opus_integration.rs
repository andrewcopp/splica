//! Integration tests for Opus encode → decode round-trip.
//!
//! Uses libopus (via the `opus` crate) for both encoding and decoding.
//! Verifies that frames survive the encode→decode cycle with correct
//! sample counts and audio properties.

#![cfg(feature = "codec-opus")]

use bytes::Bytes;
use splica_codec::{OpusDecoder, OpusEncoderBuilder};
use splica_core::media::{AudioFrame, ChannelLayout, Packet, SampleFormat, TrackIndex};
use splica_core::timestamp::Timestamp;
use splica_core::{AudioDecoder, AudioEncoder};

/// Creates a synthetic audio frame containing a 440 Hz sine wave.
///
/// Opus standard frame size is 960 samples at 48000 Hz (20ms).
fn make_opus_sine_frame(
    sample_rate: u32,
    channel_layout: ChannelLayout,
    pts_tick: i64,
) -> AudioFrame {
    let sample_count = 960u32;
    let channels = channel_layout.channel_count();
    let total_samples = sample_count * channels;
    let mut data = Vec::with_capacity(total_samples as usize * 4);
    for i in 0..total_samples {
        let t = i as f32 / (sample_rate * channels) as f32;
        let sample = (t * std::f32::consts::TAU * 440.0).sin() * 0.5;
        data.extend_from_slice(&sample.to_le_bytes());
    }

    AudioFrame {
        sample_rate,
        channel_layout,
        sample_format: SampleFormat::F32,
        sample_count,
        pts: Timestamp::new(pts_tick, sample_rate).unwrap(),
        data: vec![Bytes::from(data)],
    }
}

/// Encodes N Opus frames and returns the encoded packets.
fn encode_opus_frames(
    sample_rate: u32,
    channel_layout: ChannelLayout,
    count: usize,
) -> Vec<Packet> {
    let mut encoder = OpusEncoderBuilder::new()
        .sample_rate(sample_rate)
        .channel_layout(channel_layout)
        .build()
        .unwrap();

    let mut packets = Vec::new();

    for i in 0..count {
        let frame = make_opus_sine_frame(sample_rate, channel_layout, i as i64 * 960);
        encoder.send_frame(Some(&frame)).unwrap();
        if let Some(pkt) = encoder.receive_packet().unwrap() {
            packets.push(pkt);
        }
    }

    // Flush encoder
    encoder.send_frame(None).unwrap();
    if let Some(pkt) = encoder.receive_packet().unwrap() {
        packets.push(pkt);
    }

    packets
}

#[test]
fn test_that_opus_encoder_decoder_round_trip_preserves_frame_count() {
    // GIVEN — encode 20 Opus frames at 48000 Hz stereo
    let frame_count = 20;
    let encoded_packets = encode_opus_frames(48000, ChannelLayout::Stereo, frame_count);

    // Opus produces one packet per frame immediately
    assert_eq!(
        encoded_packets.len(),
        frame_count,
        "Opus encoder should produce one packet per frame"
    );

    // WHEN — decode all encoded packets
    let mut decoder = OpusDecoder::new(48000, ChannelLayout::Stereo).unwrap();
    let mut decoded_count = 0u32;

    for pkt in &encoded_packets {
        let decode_pkt = Packet {
            track_index: TrackIndex(0),
            pts: pkt.pts,
            dts: pkt.dts,
            is_keyframe: true,
            data: pkt.data.clone(),
        };

        decoder.send_packet(Some(&decode_pkt)).unwrap();
        while let Some(_frame) = decoder.receive_frame().unwrap() {
            decoded_count += 1;
        }
    }

    // Flush decoder
    decoder.send_packet(None).unwrap();
    while let Some(_frame) = decoder.receive_frame().unwrap() {
        decoded_count += 1;
    }

    // THEN — decoded frame count matches encoded packet count
    assert_eq!(
        decoded_count,
        encoded_packets.len() as u32,
        "decoded frame count ({decoded_count}) should match encoded packet count ({})",
        encoded_packets.len()
    );
}

#[test]
fn test_that_opus_decoded_frames_have_correct_sample_count() {
    // GIVEN — encode one Opus frame (960 samples at 48000 Hz)
    let encoded_packets = encode_opus_frames(48000, ChannelLayout::Stereo, 1);

    assert_eq!(encoded_packets.len(), 1);

    // WHEN — decode it
    let mut decoder = OpusDecoder::new(48000, ChannelLayout::Stereo).unwrap();
    let pkt = &encoded_packets[0];
    let decode_pkt = Packet {
        track_index: TrackIndex(0),
        pts: pkt.pts,
        dts: pkt.dts,
        is_keyframe: true,
        data: pkt.data.clone(),
    };

    decoder.send_packet(Some(&decode_pkt)).unwrap();
    let frame = decoder.receive_frame().unwrap();

    // THEN — decoded frame has 960 samples per channel
    let audio_frame = frame.expect("should decode one frame");
    assert_eq!(audio_frame.sample_count, 960);
    assert_eq!(audio_frame.sample_rate, 48000);
    assert_eq!(audio_frame.channel_layout, ChannelLayout::Stereo);
    assert_eq!(audio_frame.sample_format, SampleFormat::F32);
}

#[test]
fn test_that_opus_mono_round_trip_preserves_channel_layout() {
    // GIVEN — encode 5 mono Opus frames
    let encoded_packets = encode_opus_frames(48000, ChannelLayout::Mono, 5);

    assert_eq!(encoded_packets.len(), 5);

    // WHEN — decode first packet
    let mut decoder = OpusDecoder::new(48000, ChannelLayout::Mono).unwrap();
    let pkt = &encoded_packets[0];
    let decode_pkt = Packet {
        track_index: TrackIndex(0),
        pts: pkt.pts,
        dts: pkt.dts,
        is_keyframe: true,
        data: pkt.data.clone(),
    };

    decoder.send_packet(Some(&decode_pkt)).unwrap();
    let frame = decoder.receive_frame().unwrap();

    // THEN — decoded frame is mono
    let audio_frame = frame.expect("should decode one frame");
    assert_eq!(audio_frame.channel_layout, ChannelLayout::Mono);
    assert_eq!(audio_frame.sample_count, 960);
}

#[test]
fn test_that_opus_round_trip_produces_nonzero_audio_data() {
    // GIVEN — encode a sine wave
    let encoded_packets = encode_opus_frames(48000, ChannelLayout::Stereo, 3);

    // WHEN — decode third frame (avoiding any initial codec warmup artifacts)
    let mut decoder = OpusDecoder::new(48000, ChannelLayout::Stereo).unwrap();
    let mut last_frame: Option<AudioFrame> = None;

    for pkt in &encoded_packets {
        let decode_pkt = Packet {
            track_index: TrackIndex(0),
            pts: pkt.pts,
            dts: pkt.dts,
            is_keyframe: true,
            data: pkt.data.clone(),
        };

        decoder.send_packet(Some(&decode_pkt)).unwrap();
        if let Some(frame) = decoder.receive_frame().unwrap() {
            last_frame = Some(frame);
        }
    }

    // THEN — decoded audio data is not all zeros
    let frame = last_frame.expect("should decode at least one frame");
    let data = &frame.data[0];
    let has_nonzero = data.iter().any(|&b| b != 0);
    assert!(has_nonzero, "decoded audio data should not be all zeros");
}
