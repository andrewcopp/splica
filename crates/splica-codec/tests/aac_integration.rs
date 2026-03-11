//! Integration tests for AAC encode → decode round-trip.
//!
//! Uses fdk-aac to encode synthetic PCM audio, then decodes with symphonia's
//! AAC decoder and verifies the output frames.

#![cfg(all(feature = "codec-aac", feature = "codec-aac-enc"))]

use bytes::Bytes;
use splica_codec::{AacDecoder, AacEncoderBuilder};
use splica_core::media::{AudioFrame, ChannelLayout, Packet, SampleFormat, TrackIndex};
use splica_core::timestamp::Timestamp;
use splica_core::{AudioDecoder, AudioEncoder};

/// Creates a synthetic stereo audio frame containing a 440 Hz sine wave.
///
/// AAC-LC frame size is 1024 samples per channel.
fn make_sine_frame(sample_rate: u32, channels: u32, pts_tick: i64) -> AudioFrame {
    let sample_count = 1024u32;
    let total_samples = sample_count * channels;
    let mut data = Vec::with_capacity(total_samples as usize * 4);
    for i in 0..total_samples {
        let t = i as f32 / (sample_rate * channels) as f32;
        let sample = (t * std::f32::consts::TAU * 440.0).sin() * 0.5;
        data.extend_from_slice(&sample.to_le_bytes());
    }

    AudioFrame {
        sample_rate,
        channel_layout: if channels == 1 {
            ChannelLayout::Mono
        } else {
            ChannelLayout::Stereo
        },
        sample_format: SampleFormat::F32,
        sample_count,
        pts: Timestamp::new(pts_tick, sample_rate).unwrap(),
        data: vec![Bytes::from(data)],
    }
}

/// Encodes N audio frames and returns (AudioSpecificConfig, Vec<encoded packets>).
fn encode_audio_frames(
    sample_rate: u32,
    channel_layout: ChannelLayout,
    count: usize,
) -> (Vec<u8>, Vec<Packet>) {
    let channels = channel_layout.channel_count();
    let mut encoder = AacEncoderBuilder::new()
        .sample_rate(sample_rate)
        .channel_layout(channel_layout)
        .build()
        .unwrap();

    let asc = encoder.audio_specific_config();
    let mut packets = Vec::new();

    for i in 0..count {
        let frame = make_sine_frame(sample_rate, channels, i as i64 * 1024);
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

    (asc, packets)
}

#[test]
fn test_that_aac_encoder_decoder_round_trip_preserves_frame_count() {
    // GIVEN — encode 10 AAC frames at 44100 Hz stereo
    let frame_count = 10;
    let (asc, encoded_packets) = encode_audio_frames(44100, ChannelLayout::Stereo, frame_count);

    // fdk-aac may need a few frames before producing output, so we may get
    // fewer packets than input frames. Use the actual packet count as baseline.
    assert!(
        !encoded_packets.is_empty(),
        "encoder should produce at least one packet"
    );

    // WHEN — decode all encoded packets
    let mut decoder = AacDecoder::new(&asc).unwrap();
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
fn test_that_aac_decoded_frames_have_correct_sample_format() {
    // GIVEN — encode a few AAC frames
    let (asc, encoded_packets) = encode_audio_frames(44100, ChannelLayout::Stereo, 5);

    assert!(
        !encoded_packets.is_empty(),
        "encoder should produce at least one packet"
    );

    // WHEN — decode the first available packet
    let mut decoder = AacDecoder::new(&asc).unwrap();
    let mut decoded_frame: Option<AudioFrame> = None;

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
            decoded_frame = Some(frame);
            break;
        }
    }

    // THEN — decoded frame has expected audio properties
    let frame = decoded_frame.expect("should decode at least one frame");
    assert_eq!(frame.sample_rate, 44100);
    assert_eq!(frame.channel_layout, ChannelLayout::Stereo);
    assert_eq!(frame.sample_format, SampleFormat::F32);
    assert!(frame.sample_count > 0);
    assert!(!frame.data.is_empty());
}

#[test]
fn test_that_aac_mono_round_trip_produces_mono_output() {
    // GIVEN — encode mono AAC frames at 48000 Hz
    let (asc, encoded_packets) = encode_audio_frames(48000, ChannelLayout::Mono, 5);

    assert!(
        !encoded_packets.is_empty(),
        "encoder should produce at least one packet"
    );

    // WHEN — decode first available packet
    let mut decoder = AacDecoder::new(&asc).unwrap();
    let mut decoded_frame: Option<AudioFrame> = None;

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
            decoded_frame = Some(frame);
            break;
        }
    }

    // THEN — decoded frame is mono at 48000 Hz
    let frame = decoded_frame.expect("should decode at least one frame");
    assert_eq!(frame.sample_rate, 48000);
    assert_eq!(frame.channel_layout, ChannelLayout::Mono);
}
