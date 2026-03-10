//! High-level pipeline orchestration connecting demux, decode, filter, encode, and mux.
//!
//! The pipeline drives the canonical media processing loop:
//! demux → decode → encode → mux, with optional event callbacks for progress
//! reporting. Tracks without a configured decoder/encoder are passed through
//! in copy mode (compressed packets go directly from demuxer to muxer).

pub mod event;

use std::collections::HashMap;

pub use event::{PipelineEvent, PipelineEventKind};
use splica_core::{Decoder, Demuxer, Encoder, Muxer, PipelineError, TrackIndex};

/// How the pipeline should handle a track.
#[allow(dead_code)] // Copy variant used once stream-copy mode is wired up
enum TrackMode {
    /// Decode → encode: full transcode path.
    Transcode {
        decoder: Box<dyn Decoder>,
        encoder: Box<dyn Encoder>,
    },
    /// Copy: pass compressed packets directly to the muxer.
    Copy,
}

/// Builder for configuring and running a media processing pipeline.
///
/// Accepts an optional event callback for structured progress reporting.
/// The callback receives [`PipelineEvent`] values as the pipeline executes.
///
/// # Example
///
/// ```
/// use splica_pipeline::{PipelineBuilder, PipelineEvent, PipelineEventKind};
///
/// let builder = PipelineBuilder::new().with_event_handler(|event: PipelineEvent| {
///     match event.kind {
///         PipelineEventKind::PacketsRead { count } => {
///             println!("Read {} packets", count);
///         }
///         _ => {}
///     }
/// });
/// ```
pub struct PipelineBuilder<F = fn(PipelineEvent)> {
    on_event: Option<F>,
    demuxer: Option<Box<dyn Demuxer>>,
    muxer: Option<Box<dyn Muxer>>,
    decoders: HashMap<TrackIndex, Box<dyn Decoder>>,
    encoders: HashMap<TrackIndex, Box<dyn Encoder>>,
}

impl PipelineBuilder {
    /// Creates a new pipeline builder with no event callback.
    pub fn new() -> Self {
        Self {
            on_event: None,
            demuxer: None,
            muxer: None,
            decoders: HashMap::new(),
            encoders: HashMap::new(),
        }
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl<F: Fn(PipelineEvent)> PipelineBuilder<F> {
    /// Sets a callback that receives pipeline events for progress reporting.
    pub fn with_event_handler<G: Fn(PipelineEvent)>(self, handler: G) -> PipelineBuilder<G> {
        PipelineBuilder {
            on_event: Some(handler),
            demuxer: self.demuxer,
            muxer: self.muxer,
            decoders: self.decoders,
            encoders: self.encoders,
        }
    }

    /// Sets the demuxer (input source).
    pub fn with_demuxer(mut self, demuxer: impl Demuxer + 'static) -> Self {
        self.demuxer = Some(Box::new(demuxer));
        self
    }

    /// Sets the muxer (output destination).
    pub fn with_muxer(mut self, muxer: impl Muxer + 'static) -> Self {
        self.muxer = Some(Box::new(muxer));
        self
    }

    /// Registers a decoder for a specific track.
    ///
    /// Tracks without a decoder will be passed through in copy mode.
    pub fn with_decoder(
        mut self,
        track: TrackIndex,
        decoder: impl Decoder + 'static,
    ) -> Self {
        self.decoders.insert(track, Box::new(decoder));
        self
    }

    /// Registers an encoder for a specific track.
    ///
    /// Must be paired with a decoder for the same track.
    pub fn with_encoder(
        mut self,
        track: TrackIndex,
        encoder: impl Encoder + 'static,
    ) -> Self {
        self.encoders.insert(track, Box::new(encoder));
        self
    }

    /// Builds and returns a configured [`Pipeline`] ready to run.
    ///
    /// Returns an error if the configuration is invalid (missing demuxer/muxer,
    /// encoder without matching decoder, etc.).
    pub fn build(mut self) -> Result<Pipeline<F>, PipelineError> {
        let demuxer = self.demuxer.take().ok_or(PipelineError::Config {
            message: "pipeline requires a demuxer".to_string(),
        })?;
        let muxer = self.muxer.take().ok_or(PipelineError::Config {
            message: "pipeline requires a muxer".to_string(),
        })?;

        // Validate: every encoder must have a matching decoder
        for track in self.encoders.keys() {
            if !self.decoders.contains_key(track) {
                return Err(PipelineError::Config {
                    message: format!(
                        "track {} has an encoder but no decoder — both are required for transcoding",
                        track.0
                    ),
                });
            }
        }

        // Validate: every decoder must have a matching encoder
        for track in self.decoders.keys() {
            if !self.encoders.contains_key(track) {
                return Err(PipelineError::Config {
                    message: format!(
                        "track {} has a decoder but no encoder — both are required for transcoding",
                        track.0
                    ),
                });
            }
        }

        // Build track modes
        let mut track_modes: HashMap<TrackIndex, TrackMode> = HashMap::new();
        for (track, decoder) in self.decoders.drain() {
            let encoder = self.encoders.remove(&track).expect("validated above");
            track_modes.insert(track, TrackMode::Transcode { decoder, encoder });
        }

        Ok(Pipeline {
            demuxer,
            muxer,
            track_modes,
            on_event: self.on_event,
        })
    }

    /// Emits an event to the registered handler, if any.
    #[cfg(test)]
    pub(crate) fn emit(&self, event: PipelineEvent) {
        if let Some(ref f) = self.on_event {
            f(event);
        }
    }
}

/// A configured media processing pipeline ready to execute.
///
/// Created via [`PipelineBuilder::build()`]. Call [`run()`](Pipeline::run) to
/// process all packets from the demuxer through to the muxer.
pub struct Pipeline<F = fn(PipelineEvent)> {
    demuxer: Box<dyn Demuxer>,
    muxer: Box<dyn Muxer>,
    track_modes: HashMap<TrackIndex, TrackMode>,
    on_event: Option<F>,
}

fn emit_event<F: Fn(PipelineEvent)>(on_event: &Option<F>, kind: PipelineEventKind) {
    if let Some(ref f) = on_event {
        f(PipelineEvent::new(kind));
    }
}

/// Drains all available frames from a decoder, encodes them, and writes
/// resulting packets to the muxer.
fn drain_decoder_to_muxer<F: Fn(PipelineEvent)>(
    decoder: &mut dyn Decoder,
    encoder: &mut dyn Encoder,
    muxer: &mut dyn Muxer,
    output_track: TrackIndex,
    on_event: &Option<F>,
    counters: &mut PipelineCounters,
) -> Result<(), PipelineError> {
    while let Some(frame) = decoder.receive_frame()? {
        counters.frames_decoded += 1;
        emit_event(on_event, PipelineEventKind::FramesDecoded {
            count: counters.frames_decoded,
        });

        encoder.send_frame(Some(&frame))?;
        drain_encoder_to_muxer(encoder, muxer, output_track, on_event, counters)?;
    }
    Ok(())
}

/// Drains all available packets from an encoder and writes them to the muxer.
fn drain_encoder_to_muxer<F: Fn(PipelineEvent)>(
    encoder: &mut dyn Encoder,
    muxer: &mut dyn Muxer,
    output_track: TrackIndex,
    on_event: &Option<F>,
    counters: &mut PipelineCounters,
) -> Result<(), PipelineError> {
    while let Some(mut encoded_packet) = encoder.receive_packet()? {
        counters.frames_encoded += 1;
        emit_event(on_event, PipelineEventKind::FramesEncoded {
            count: counters.frames_encoded,
        });

        encoded_packet.track_index = output_track;
        muxer.write_packet(&encoded_packet)?;
        counters.packets_written += 1;
        emit_event(on_event, PipelineEventKind::PacketsWritten {
            count: counters.packets_written,
        });
    }
    Ok(())
}

struct PipelineCounters {
    packets_read: u64,
    frames_decoded: u64,
    frames_encoded: u64,
    packets_written: u64,
}

impl<F: Fn(PipelineEvent)> Pipeline<F> {
    /// Runs the pipeline to completion.
    ///
    /// Processes all packets from the demuxer, routing each through the
    /// appropriate decode→encode path or copy mode, then writes to the muxer.
    /// Flushes decoders and encoders at end-of-stream and finalizes the muxer.
    pub fn run(&mut self) -> Result<(), PipelineError> {
        // Register all input tracks with the muxer
        let tracks = self.demuxer.tracks().to_vec();
        let mut input_to_output: HashMap<TrackIndex, TrackIndex> = HashMap::new();
        for track_info in &tracks {
            let output_idx = self.muxer.add_track(track_info)?;
            input_to_output.insert(track_info.index, output_idx);
        }

        let mut counters = PipelineCounters {
            packets_read: 0,
            frames_decoded: 0,
            frames_encoded: 0,
            packets_written: 0,
        };

        // Main demux loop
        while let Some(packet) = self.demuxer.read_packet()? {
            counters.packets_read += 1;
            emit_event(&self.on_event, PipelineEventKind::PacketsRead {
                count: counters.packets_read,
            });

            let input_track = packet.track_index;
            let output_track = input_to_output.get(&input_track).copied().ok_or(
                PipelineError::Config {
                    message: format!(
                        "packet references track {} which was not in demuxer tracks",
                        input_track.0
                    ),
                },
            )?;

            match self.track_modes.get_mut(&input_track) {
                Some(TrackMode::Transcode { decoder, encoder }) => {
                    decoder.send_packet(Some(&packet))?;
                    drain_decoder_to_muxer(
                        decoder.as_mut(),
                        encoder.as_mut(),
                        self.muxer.as_mut(),
                        output_track,
                        &self.on_event,
                        &mut counters,
                    )?;
                }
                Some(TrackMode::Copy) | None => {
                    // Copy mode: pass packet directly to muxer
                    let mut out_packet = packet;
                    out_packet.track_index = output_track;
                    self.muxer.write_packet(&out_packet)?;
                    counters.packets_written += 1;
                    emit_event(&self.on_event, PipelineEventKind::PacketsWritten {
                        count: counters.packets_written,
                    });
                }
            }
        }

        // Flush decoders and encoders at end-of-stream
        let track_indices: Vec<TrackIndex> = self.track_modes.keys().copied().collect();
        for track_idx in track_indices {
            let output_track = input_to_output[&track_idx];
            if let Some(TrackMode::Transcode { decoder, encoder }) =
                self.track_modes.get_mut(&track_idx)
            {
                // Signal end-of-stream to decoder, drain remaining frames
                decoder.send_packet(None)?;
                drain_decoder_to_muxer(
                    decoder.as_mut(),
                    encoder.as_mut(),
                    self.muxer.as_mut(),
                    output_track,
                    &self.on_event,
                    &mut counters,
                )?;

                // Signal end-of-stream to encoder, drain remaining packets
                encoder.send_frame(None)?;
                drain_encoder_to_muxer(
                    encoder.as_mut(),
                    self.muxer.as_mut(),
                    output_track,
                    &self.on_event,
                    &mut counters,
                )?;
            }
        }

        // Finalize the muxer
        self.muxer.finalize()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use splica_core::{
        ColorSpace, DecodeError, DemuxError, EncodeError, Frame, MuxError, Packet, PixelFormat,
        PlaneLayout, Timestamp, TrackInfo, TrackKind, VideoCodec, VideoFrame, VideoTrackInfo,
        Codec, FrameRate,
    };
    use bytes::Bytes;
    use std::any::Any;
    use std::sync::{Arc, Mutex};

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
                        ColorSpace::BT709,
                        p.pts,
                        Bytes::from(vec![0u8; 6]), // Y(1) + U(1) + V(1) padded
                        vec![
                            PlaneLayout { offset: 0, stride: 1, width: 1, height: 1 },
                            PlaneLayout { offset: 1, stride: 1, width: 1, height: 1 },
                            PlaneLayout { offset: 2, stride: 1, width: 1, height: 1 },
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

        fn as_any(&self) -> &dyn Any { self }
        fn as_any_mut(&mut self) -> &mut dyn Any { self }
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

        fn as_any(&self) -> &dyn Any { self }
        fn as_any_mut(&mut self) -> &mut dyn Any { self }
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
                frame_rate: Some(FrameRate::new(30, 1)),
            }),
            audio: None,
        }
    }

    fn make_packet(track: u32, frame_num: i64) -> Packet {
        Packet {
            track_index: TrackIndex(track),
            pts: Timestamp::new(frame_num, 30),
            dts: Timestamp::new(frame_num, 30),
            is_keyframe: frame_num == 0,
            data: Bytes::from(vec![0u8; 100]),
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
        assert!(collected.iter().any(|k| matches!(k, PipelineEventKind::PacketsRead { .. })));
        assert!(collected.iter().any(|k| matches!(k, PipelineEventKind::FramesDecoded { .. })));
        assert!(collected.iter().any(|k| matches!(k, PipelineEventKind::FramesEncoded { .. })));
        assert!(collected.iter().any(|k| matches!(k, PipelineEventKind::PacketsWritten { .. })));
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
}
