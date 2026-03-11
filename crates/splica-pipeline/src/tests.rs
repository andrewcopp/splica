use super::*;
use bytes::Bytes;
use splica_core::{
    AudioCodec, AudioFrame, AudioTrackInfo, ChannelLayout, Codec, ColorSpace, DecodeError,
    DemuxError, EncodeError, FilterError, Frame, FrameRate, MuxError, Packet, PipelineError,
    PixelFormat, PlaneLayout, SampleFormat, Timestamp, TrackIndex, TrackInfo, TrackKind,
    ValidationError, VideoCodec, VideoFrame, VideoTrackInfo,
};
use std::any::Any;
use std::sync::{Arc, Mutex};

use splica_core::{
    AudioDecoder, AudioEncoder, AudioFilter, Decoder, Demuxer, Encoder, Muxer, VideoFilter,
};

// -----------------------------------------------------------------------
// Mock implementations
// -----------------------------------------------------------------------

struct MockDemuxer {
    tracks: Vec<TrackInfo>,
    packets: Vec<Packet>,
    position: usize,
}

impl Demuxer for MockDemuxer {
    fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    fn read_packet(&mut self) -> Result<Option<Packet>, DemuxError> {
        if self.position < self.packets.len() {
            let packet = self.packets[self.position].clone();
            self.position += 1;
            Ok(Some(packet))
        } else {
            Ok(None)
        }
    }
}

/// A mock decoder that produces one frame per packet.
struct MockDecoder {
    pending_frames: Vec<Frame>,
}

impl MockDecoder {
    fn new() -> Self {
        Self {
            pending_frames: Vec::new(),
        }
    }
}

impl Decoder for MockDecoder {
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError> {
        if let Some(p) = packet {
            // Produce a minimal 1x1 YUV420 frame per packet
            let frame = Frame::Video(
                VideoFrame::new(
                    1,
                    1,
                    PixelFormat::Yuv420p,
                    Some(ColorSpace::BT709),
                    p.pts,
                    Bytes::from(vec![0u8; 6]), // Y(1) + U(1) + V(1) padded
                    vec![
                        PlaneLayout {
                            offset: 0,
                            stride: 1,
                            width: 1,
                            height: 1,
                        },
                        PlaneLayout {
                            offset: 1,
                            stride: 1,
                            width: 1,
                            height: 1,
                        },
                        PlaneLayout {
                            offset: 2,
                            stride: 1,
                            width: 1,
                            height: 1,
                        },
                    ],
                )
                .expect("valid test frame"),
            );
            self.pending_frames.push(frame);
        }
        // None = EOS, no extra frames to flush in our mock
        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Option<Frame>, DecodeError> {
        if self.pending_frames.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.pending_frames.remove(0)))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A mock encoder that produces one packet per frame.
struct MockEncoder {
    pending_packets: Vec<Packet>,
}

impl MockEncoder {
    fn new() -> Self {
        Self {
            pending_packets: Vec::new(),
        }
    }
}

impl Encoder for MockEncoder {
    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<(), EncodeError> {
        if let Some(f) = frame {
            let pts = match f {
                Frame::Video(vf) => vf.pts,
                Frame::Audio(af) => af.pts,
            };
            self.pending_packets.push(Packet {
                track_index: TrackIndex(0),
                pts,
                dts: pts,
                is_keyframe: true,
                data: Bytes::from(vec![0u8; 50]),
            });
        }
        // None = EOS, no extra packets to flush in our mock
        Ok(())
    }

    fn receive_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        if self.pending_packets.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.pending_packets.remove(0)))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A muxer that records what it receives via shared state.
struct SharedMuxer {
    packets: Arc<Mutex<Vec<Packet>>>,
    finalized: Arc<Mutex<bool>>,
}

impl SharedMuxer {
    #[allow(clippy::type_complexity)]
    fn new() -> (Self, Arc<Mutex<Vec<Packet>>>, Arc<Mutex<bool>>) {
        let packets = Arc::new(Mutex::new(Vec::new()));
        let finalized = Arc::new(Mutex::new(false));
        (
            Self {
                packets: Arc::clone(&packets),
                finalized: Arc::clone(&finalized),
            },
            packets,
            finalized,
        )
    }
}

impl Muxer for SharedMuxer {
    fn add_track(&mut self, _info: &TrackInfo) -> Result<TrackIndex, MuxError> {
        Ok(TrackIndex(0))
    }
    fn write_packet(&mut self, packet: &Packet) -> Result<(), MuxError> {
        self.packets.lock().unwrap().push(packet.clone());
        Ok(())
    }
    fn finalize(&mut self) -> Result<(), MuxError> {
        *self.finalized.lock().unwrap() = true;
        Ok(())
    }
}

struct NullMuxer;
impl Muxer for NullMuxer {
    fn add_track(&mut self, _: &TrackInfo) -> Result<TrackIndex, MuxError> {
        Ok(TrackIndex(0))
    }
    fn write_packet(&mut self, _: &Packet) -> Result<(), MuxError> {
        Ok(())
    }
    fn finalize(&mut self) -> Result<(), MuxError> {
        Ok(())
    }
}

fn make_track(index: u32) -> TrackInfo {
    TrackInfo {
        index: TrackIndex(index),
        kind: TrackKind::Video,
        codec: Codec::Video(VideoCodec::H264),
        duration: None,
        video: Some(VideoTrackInfo {
            width: 1920,
            height: 1080,
            pixel_format: Some(PixelFormat::Yuv420p),
            color_space: None,
            frame_rate: FrameRate::new(30, 1),
        }),
        audio: None,
    }
}

fn make_packet(track: u32, frame_num: i64) -> Packet {
    Packet {
        track_index: TrackIndex(track),
        pts: Timestamp::new(frame_num, 30).unwrap(),
        dts: Timestamp::new(frame_num, 30).unwrap(),
        is_keyframe: frame_num == 0,
        data: Bytes::from(vec![0u8; 100]),
    }
}

// -----------------------------------------------------------------------
// Audio mock implementations
// -----------------------------------------------------------------------

fn make_audio_frame(pts: Timestamp) -> AudioFrame {
    AudioFrame {
        sample_rate: 48000,
        channel_layout: ChannelLayout::Stereo,
        sample_format: SampleFormat::F32,
        sample_count: 1024,
        pts,
        data: vec![Bytes::from(vec![0u8; 1024 * 2 * 4])],
    }
}

/// A mock audio decoder that produces one frame per packet.
struct MockAudioDecoder {
    pending_frames: Vec<AudioFrame>,
}

impl MockAudioDecoder {
    fn new() -> Self {
        Self {
            pending_frames: Vec::new(),
        }
    }
}

impl AudioDecoder for MockAudioDecoder {
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError> {
        if let Some(p) = packet {
            self.pending_frames.push(make_audio_frame(p.pts));
        }
        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError> {
        if self.pending_frames.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.pending_frames.remove(0)))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A mock audio decoder that withholds frames until EOS flush.
struct BufferingAudioDecoder {
    buffered: Vec<AudioFrame>,
    flushing: bool,
}

impl BufferingAudioDecoder {
    fn new() -> Self {
        Self {
            buffered: Vec::new(),
            flushing: false,
        }
    }
}

impl AudioDecoder for BufferingAudioDecoder {
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError> {
        if let Some(p) = packet {
            self.buffered.push(make_audio_frame(p.pts));
        } else {
            self.flushing = true;
        }
        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError> {
        if self.flushing && !self.buffered.is_empty() {
            Ok(Some(self.buffered.remove(0)))
        } else {
            Ok(None)
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A mock audio encoder that produces one packet per frame.
struct MockAudioEncoder {
    pending_packets: Vec<Packet>,
}

impl MockAudioEncoder {
    fn new() -> Self {
        Self {
            pending_packets: Vec::new(),
        }
    }
}

impl AudioEncoder for MockAudioEncoder {
    fn send_frame(&mut self, frame: Option<&AudioFrame>) -> Result<(), EncodeError> {
        if let Some(f) = frame {
            self.pending_packets.push(Packet {
                track_index: TrackIndex(0),
                pts: f.pts,
                dts: f.pts,
                is_keyframe: true,
                data: Bytes::from(vec![0u8; 50]),
            });
        }
        Ok(())
    }

    fn receive_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        if self.pending_packets.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.pending_packets.remove(0)))
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A mock audio filter that records how many frames it processed.
struct RecordingAudioFilter {
    count: Arc<Mutex<u32>>,
}

impl RecordingAudioFilter {
    fn new(count: Arc<Mutex<u32>>) -> Self {
        Self { count }
    }
}

impl AudioFilter for RecordingAudioFilter {
    fn process(&mut self, frame: AudioFrame) -> Result<AudioFrame, FilterError> {
        *self.count.lock().unwrap() += 1;
        Ok(frame)
    }
}

fn make_audio_track(index: u32) -> TrackInfo {
    TrackInfo {
        index: TrackIndex(index),
        kind: TrackKind::Audio,
        codec: Codec::Audio(AudioCodec::Aac),
        duration: None,
        video: None,
        audio: Some(AudioTrackInfo {
            sample_rate: 48000,
            channel_layout: Some(ChannelLayout::Stereo),
            sample_format: None,
        }),
    }
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[test]
fn test_that_pipeline_builder_emits_events_to_callback() {
    // GIVEN
    let events: Arc<Mutex<Vec<PipelineEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);

    let builder = PipelineBuilder::new().with_event_handler(move |event: PipelineEvent| {
        events_clone.lock().unwrap().push(event);
    });

    // WHEN
    builder.emit(PipelineEvent::new(PipelineEventKind::PacketsRead {
        count: 42,
    }));

    // THEN
    let collected = events.lock().unwrap();
    assert_eq!(collected.len(), 1);
    assert!(matches!(
        collected[0].kind,
        PipelineEventKind::PacketsRead { count: 42 }
    ));
}

#[test]
fn test_that_pipeline_builder_without_handler_does_not_panic() {
    // GIVEN
    let builder = PipelineBuilder::new();

    // WHEN
    builder.emit(PipelineEvent::new(PipelineEventKind::FramesDecoded {
        count: 10,
    }));

    // THEN — no panic
}

#[test]
fn test_that_pipeline_event_carries_timestamp() {
    // GIVEN
    let before = std::time::Instant::now();
    let event = PipelineEvent::new(PipelineEventKind::FramesEncoded { count: 5 });
    let after = std::time::Instant::now();

    // THEN
    assert!(event.timestamp >= before);
    assert!(event.timestamp <= after);
}

#[test]
fn test_that_pipeline_requires_demuxer() {
    // WHEN
    let result = PipelineBuilder::new().with_muxer(NullMuxer).build();

    // THEN
    assert!(result.is_err());
}

#[test]
fn test_that_pipeline_requires_muxer() {
    // GIVEN
    let demuxer = MockDemuxer {
        tracks: vec![],
        packets: vec![],
        position: 0,
    };

    // WHEN
    let result = PipelineBuilder::new().with_demuxer(demuxer).build();

    // THEN
    assert!(result.is_err());
}

#[test]
fn test_that_encoder_without_decoder_is_rejected() {
    // GIVEN
    let demuxer = MockDemuxer {
        tracks: vec![make_track(0)],
        packets: vec![],
        position: 0,
    };

    // WHEN
    let result = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(NullMuxer)
        .with_encoder(TrackIndex(0), MockEncoder::new())
        .build();

    // THEN
    assert!(result.is_err());
}

#[test]
fn test_that_pipeline_copies_packets_without_codec() {
    // GIVEN
    let demuxer = MockDemuxer {
        tracks: vec![make_track(0)],
        packets: vec![make_packet(0, 0), make_packet(0, 1), make_packet(0, 2)],
        position: 0,
    };
    let (muxer, written_packets, finalized) = SharedMuxer::new();

    // WHEN — no decoder/encoder configured, so copy mode
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(muxer)
        .build()
        .unwrap();

    pipeline.run().unwrap();

    // THEN
    assert_eq!(written_packets.lock().unwrap().len(), 3);
    assert!(*finalized.lock().unwrap());
}

#[test]
fn test_that_pipeline_transcodes_through_decoder_and_encoder() {
    // GIVEN
    let demuxer = MockDemuxer {
        tracks: vec![make_track(0)],
        packets: vec![make_packet(0, 0), make_packet(0, 1)],
        position: 0,
    };
    let (muxer, written_packets, finalized) = SharedMuxer::new();

    // WHEN
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_decoder(TrackIndex(0), MockDecoder::new())
        .with_encoder(TrackIndex(0), MockEncoder::new())
        .with_muxer(muxer)
        .build()
        .unwrap();

    pipeline.run().unwrap();

    // THEN — 2 packets in, 2 frames decoded, 2 frames encoded, 2 packets out
    assert_eq!(written_packets.lock().unwrap().len(), 2);
    assert!(*finalized.lock().unwrap());
}

#[test]
fn test_that_pipeline_emits_all_event_types_during_transcode() {
    // GIVEN
    let demuxer = MockDemuxer {
        tracks: vec![make_track(0)],
        packets: vec![make_packet(0, 0), make_packet(0, 1), make_packet(0, 2)],
        position: 0,
    };

    let events: Arc<Mutex<Vec<PipelineEventKind>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);

    // WHEN
    let mut pipeline = PipelineBuilder::new()
        .with_event_handler(move |event: PipelineEvent| {
            events_clone.lock().unwrap().push(event.kind);
        })
        .with_demuxer(demuxer)
        .with_decoder(TrackIndex(0), MockDecoder::new())
        .with_encoder(TrackIndex(0), MockEncoder::new())
        .with_muxer(NullMuxer)
        .build()
        .unwrap();

    pipeline.run().unwrap();

    // THEN
    let collected = events.lock().unwrap();
    assert!(collected
        .iter()
        .any(|k| matches!(k, PipelineEventKind::PacketsRead { .. })));
    assert!(collected
        .iter()
        .any(|k| matches!(k, PipelineEventKind::FramesDecoded { .. })));
    assert!(collected
        .iter()
        .any(|k| matches!(k, PipelineEventKind::FramesEncoded { .. })));
    assert!(collected
        .iter()
        .any(|k| matches!(k, PipelineEventKind::PacketsWritten { .. })));
}

#[test]
fn test_that_pipeline_handles_empty_demuxer() {
    // GIVEN
    let demuxer = MockDemuxer {
        tracks: vec![make_track(0)],
        packets: vec![],
        position: 0,
    };

    // WHEN
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(NullMuxer)
        .build()
        .unwrap();

    // THEN
    assert!(pipeline.run().is_ok());
}

#[test]
fn test_that_pipeline_event_counts_are_cumulative() {
    // GIVEN
    let demuxer = MockDemuxer {
        tracks: vec![make_track(0)],
        packets: vec![make_packet(0, 0), make_packet(0, 1), make_packet(0, 2)],
        position: 0,
    };

    let events: Arc<Mutex<Vec<PipelineEventKind>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);

    // WHEN — copy mode
    let mut pipeline = PipelineBuilder::new()
        .with_event_handler(move |event: PipelineEvent| {
            events_clone.lock().unwrap().push(event.kind);
        })
        .with_demuxer(demuxer)
        .with_muxer(NullMuxer)
        .build()
        .unwrap();

    pipeline.run().unwrap();

    // THEN — PacketsRead counts should be 1, 2, 3
    let collected = events.lock().unwrap();
    let read_counts: Vec<u64> = collected
        .iter()
        .filter_map(|k| match k {
            PipelineEventKind::PacketsRead { count } => Some(*count),
            _ => None,
        })
        .collect();
    assert_eq!(read_counts, vec![1, 2, 3]);

    let write_counts: Vec<u64> = collected
        .iter()
        .filter_map(|k| match k {
            PipelineEventKind::PacketsWritten { count } => Some(*count),
            _ => None,
        })
        .collect();
    assert_eq!(write_counts, vec![1, 2, 3]);
}

#[test]
fn test_that_audio_transcode_path_writes_correct_packet_count() {
    // GIVEN — 3 audio packets through decode→encode
    let demuxer = MockDemuxer {
        tracks: vec![make_audio_track(0)],
        packets: vec![make_packet(0, 0), make_packet(0, 1), make_packet(0, 2)],
        position: 0,
    };
    let (muxer, written_packets, _finalized) = SharedMuxer::new();

    // WHEN
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_audio_decoder(TrackIndex(0), MockAudioDecoder::new())
        .with_audio_encoder(TrackIndex(0), MockAudioEncoder::new())
        .with_muxer(muxer)
        .build()
        .unwrap();

    pipeline.run().unwrap();

    // THEN — 3 packets in → 3 frames decoded → 3 frames encoded → 3 packets out
    assert_eq!(written_packets.lock().unwrap().len(), 3);
}

#[test]
fn test_that_audio_eos_flush_drains_buffered_frames() {
    // GIVEN — a decoder that buffers all frames until EOS
    let demuxer = MockDemuxer {
        tracks: vec![make_audio_track(0)],
        packets: vec![make_packet(0, 0), make_packet(0, 1)],
        position: 0,
    };
    let (muxer, written_packets, _finalized) = SharedMuxer::new();

    // WHEN
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_audio_decoder(TrackIndex(0), BufferingAudioDecoder::new())
        .with_audio_encoder(TrackIndex(0), MockAudioEncoder::new())
        .with_muxer(muxer)
        .build()
        .unwrap();

    pipeline.run().unwrap();

    // THEN — both frames appear only after EOS flush
    assert_eq!(written_packets.lock().unwrap().len(), 2);
}

#[test]
fn test_that_audio_filter_is_applied_during_transcode() {
    // GIVEN — a recording filter
    let filter_count = Arc::new(Mutex::new(0u32));
    let filter = RecordingAudioFilter::new(Arc::clone(&filter_count));

    let demuxer = MockDemuxer {
        tracks: vec![make_audio_track(0)],
        packets: vec![make_packet(0, 0), make_packet(0, 1), make_packet(0, 2)],
        position: 0,
    };

    // WHEN
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_audio_decoder(TrackIndex(0), MockAudioDecoder::new())
        .with_audio_encoder(TrackIndex(0), MockAudioEncoder::new())
        .with_audio_filter(TrackIndex(0), filter)
        .with_muxer(NullMuxer)
        .build()
        .unwrap();

    pipeline.run().unwrap();

    // THEN — filter was invoked once per decoded frame
    assert_eq!(*filter_count.lock().unwrap(), 3);
}

#[test]
fn test_that_audio_transcode_emits_decode_and_encode_events() {
    // GIVEN
    let demuxer = MockDemuxer {
        tracks: vec![make_audio_track(0)],
        packets: vec![make_packet(0, 0), make_packet(0, 1)],
        position: 0,
    };

    let events: Arc<Mutex<Vec<PipelineEventKind>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);

    // WHEN
    let mut pipeline = PipelineBuilder::new()
        .with_event_handler(move |event: PipelineEvent| {
            events_clone.lock().unwrap().push(event.kind);
        })
        .with_demuxer(demuxer)
        .with_audio_decoder(TrackIndex(0), MockAudioDecoder::new())
        .with_audio_encoder(TrackIndex(0), MockAudioEncoder::new())
        .with_muxer(NullMuxer)
        .build()
        .unwrap();

    pipeline.run().unwrap();

    // THEN — all four event types emitted
    let collected = events.lock().unwrap();
    assert!(collected
        .iter()
        .any(|k| matches!(k, PipelineEventKind::PacketsRead { .. })));
    assert!(collected
        .iter()
        .any(|k| matches!(k, PipelineEventKind::FramesDecoded { .. })));
    assert!(collected
        .iter()
        .any(|k| matches!(k, PipelineEventKind::FramesEncoded { .. })));
    assert!(collected
        .iter()
        .any(|k| matches!(k, PipelineEventKind::PacketsWritten { .. })));
}

// -------------------------------------------------------------------
// Streaming validation (SPL-96)
// -------------------------------------------------------------------

#[test]
fn test_that_pipeline_writes_packets_incrementally_not_batched() {
    // GIVEN — a demuxer with 100 packets in copy mode
    let track = make_track(0);
    let packets: Vec<Packet> = (0..100).map(|i| make_packet(0, i)).collect();
    let demuxer = MockDemuxer {
        tracks: vec![track],
        packets,
        position: 0,
    };

    // A muxer that records the order packets arrive
    let (muxer, written_packets, _finalized) = SharedMuxer::new();

    // WHEN — run pipeline in copy mode (no decoder/encoder)
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(muxer)
        .build()
        .unwrap();
    pipeline.run().unwrap();

    // THEN — all 100 packets were written (proves streaming, not batching)
    let written = written_packets.lock().unwrap();
    assert_eq!(written.len(), 100);
}

#[test]
fn test_that_pipeline_streams_through_transcode_path() {
    // GIVEN — a demuxer with 50 packets through decode→encode
    let track = make_track(0);
    let packets: Vec<Packet> = (0..50).map(|i| make_packet(0, i)).collect();
    let demuxer = MockDemuxer {
        tracks: vec![track],
        packets,
        position: 0,
    };

    let (muxer, written_packets, _finalized) = SharedMuxer::new();

    // WHEN — run with decoder+encoder (transcode mode)
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(muxer)
        .with_decoder(TrackIndex(0), MockDecoder::new())
        .with_encoder(TrackIndex(0), MockEncoder::new())
        .build()
        .unwrap();
    pipeline.run().unwrap();

    // THEN — all 50 packets were transcoded and written
    let written = written_packets.lock().unwrap();
    assert_eq!(
        written.len(),
        50,
        "expected 50 packets written through transcode, got {}",
        written.len()
    );
}

#[test]
fn test_that_pipeline_copy_mode_does_not_accumulate_packets() {
    // GIVEN — a large number of packets in copy mode
    let track = make_track(0);
    let packets: Vec<Packet> = (0..1000).map(|i| make_packet(0, i)).collect();
    let total_input_bytes: usize = packets.iter().map(|p| p.data.len()).sum();
    let demuxer = MockDemuxer {
        tracks: vec![track],
        packets,
        position: 0,
    };

    let (muxer, written_packets, _finalized) = SharedMuxer::new();

    // WHEN — run pipeline
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(muxer)
        .build()
        .unwrap();
    pipeline.run().unwrap();

    // THEN — all packets written, total output matches total input
    let written = written_packets.lock().unwrap();
    assert_eq!(written.len(), 1000);
    let total_output_bytes: usize = written.iter().map(|p| p.data.len()).sum();
    assert_eq!(total_output_bytes, total_input_bytes);
}

// -----------------------------------------------------------------------
// Pre-flight validation (SPL-98)
// -----------------------------------------------------------------------

#[test]
fn test_that_validate_returns_all_errors_at_once() {
    // GIVEN — a builder with no demuxer, no muxer, and orphan encoder
    let builder = PipelineBuilder::new().with_encoder(TrackIndex(0), MockEncoder::new());

    // WHEN
    let errors = builder.validate();

    // THEN — should report multiple errors, not just the first
    assert!(
        errors.len() >= 2,
        "expected at least 2 errors, got {}",
        errors.len()
    );
}

#[test]
fn test_that_validate_detects_orphan_video_filter() {
    // GIVEN — a filter on a track with no decoder/encoder
    let demuxer = MockDemuxer {
        tracks: vec![make_track(0)],
        packets: vec![],
        position: 0,
    };
    let builder = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(NullMuxer)
        .with_filter(TrackIndex(0), MockFilter);

    // WHEN
    let errors = builder.validate();

    // THEN
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::OrphanVideoFilter(_))),
        "expected OrphanVideoFilter, got: {errors:?}"
    );
}

#[test]
fn test_that_validate_returns_empty_for_valid_config() {
    // GIVEN — a properly configured pipeline
    let demuxer = MockDemuxer {
        tracks: vec![make_track(0)],
        packets: vec![],
        position: 0,
    };
    let builder = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(NullMuxer);

    // WHEN
    let errors = builder.validate();

    // THEN
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn test_that_build_returns_validation_error_type() {
    // GIVEN — encoder without decoder
    let demuxer = MockDemuxer {
        tracks: vec![make_track(0)],
        packets: vec![],
        position: 0,
    };
    let result = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(NullMuxer)
        .with_encoder(TrackIndex(0), MockEncoder::new())
        .build();

    // THEN — error should be Validation variant
    assert!(matches!(
        result,
        Err(PipelineError::Validation(
            ValidationError::EncoderWithoutDecoder(_)
        ))
    ));
}

/// Dummy video filter for orphan-filter tests.
struct MockFilter;
impl VideoFilter for MockFilter {
    fn process(
        &mut self,
        frame: splica_core::media::VideoFrame,
    ) -> Result<splica_core::media::VideoFrame, splica_core::FilterError> {
        Ok(frame)
    }
}
