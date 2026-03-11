//! High-level pipeline orchestration connecting demux, decode, filter, encode, and mux.
//!
//! The pipeline drives the canonical media processing loop:
//! demux → decode → encode → mux, with optional event callbacks for progress
//! reporting. Tracks without a configured decoder/encoder are passed through
//! in copy mode (compressed packets go directly from demuxer to muxer).

pub mod event;

use std::collections::HashMap;

pub use event::{PipelineEvent, PipelineEventKind};
use splica_core::{
    AudioDecoder, AudioEncoder, AudioFilter, Decoder, Demuxer, Encoder, Frame, Muxer,
    PipelineError, TrackIndex, ValidationError, VideoFilter,
};

/// How the pipeline should handle a track.
#[allow(dead_code)] // Copy variant used once stream-copy mode is wired up
enum TrackMode {
    /// Decode → filter → encode: full video transcode path.
    Transcode {
        decoder: Box<dyn Decoder>,
        encoder: Box<dyn Encoder>,
        filters: Vec<Box<dyn VideoFilter>>,
    },
    /// Decode → filter → encode: full audio transcode path.
    AudioTranscode {
        decoder: Box<dyn AudioDecoder>,
        encoder: Box<dyn AudioEncoder>,
        filters: Vec<Box<dyn AudioFilter>>,
    },
    /// Copy: pass compressed packets directly to the muxer.
    Copy,
}

/// Builder for configuring and running a media processing pipeline.
///
/// Accepts an optional event callback for structured progress reporting.
/// The callback receives [`PipelineEvent`] values as the pipeline executes.
///
/// # Memory model
///
/// The pipeline streams data in a single pass: packets are read one at a time
/// from the demuxer, decoded into frames, passed through filters, re-encoded,
/// and written to the muxer. No full-file buffering occurs during processing.
///
/// **Bounded allocations during transcode:**
/// - One compressed packet at a time (demuxer → decoder)
/// - One decoded frame at a time (decoder → filter → encoder)
/// - Encoder lookahead buffer (codec-dependent, typically 1–4 frames for H.264,
///   up to ~35 frames for rav1e AV1 at default speed settings)
/// - One encoded packet at a time (encoder → muxer)
///
/// **Bounded allocations at open/close:**
/// - MP4 `moov` box is parsed into memory at open (bounded by `ResourceBudget`)
/// - MP4 muxer builds the `moov` box in memory at finalize (proportional to
///   the number of samples, not their data size — ~16 bytes per sample)
/// - WebM/MKV Cues element is buffered at finalize (~12 bytes per keyframe)
///
/// **What is NOT guaranteed:**
/// - Seek tables and index structures may require bounded lookahead
/// - Fragmented MP4 (fMP4) writes `moof`+`mdat` pairs incrementally, but each
///   fragment is buffered before writing
///
/// Peak RSS during transcode is proportional to the largest individual frame
/// buffer (width × height × 1.5 for YUV420p) plus codec lookahead, regardless
/// of total file size. A 100MB input and a 10GB input use approximately the
/// same peak memory.
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
    filters: HashMap<TrackIndex, Vec<Box<dyn VideoFilter>>>,
    audio_decoders: HashMap<TrackIndex, Box<dyn AudioDecoder>>,
    audio_encoders: HashMap<TrackIndex, Box<dyn AudioEncoder>>,
    audio_filters: HashMap<TrackIndex, Vec<Box<dyn AudioFilter>>>,
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
            filters: HashMap::new(),
            audio_decoders: HashMap::new(),
            audio_encoders: HashMap::new(),
            audio_filters: HashMap::new(),
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
            filters: self.filters,
            audio_decoders: self.audio_decoders,
            audio_encoders: self.audio_encoders,
            audio_filters: self.audio_filters,
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
    pub fn with_decoder(mut self, track: TrackIndex, decoder: impl Decoder + 'static) -> Self {
        self.decoders.insert(track, Box::new(decoder));
        self
    }

    /// Registers an encoder for a specific track.
    ///
    /// Must be paired with a decoder for the same track.
    pub fn with_encoder(mut self, track: TrackIndex, encoder: impl Encoder + 'static) -> Self {
        self.encoders.insert(track, Box::new(encoder));
        self
    }

    /// Registers a video filter for a specific track.
    ///
    /// Filters are applied in order after decode and before encode.
    /// Multiple filters can be added per track by calling this multiple times.
    /// Only applies to tracks in transcode mode (with decoder + encoder).
    pub fn with_filter(mut self, track: TrackIndex, filter: impl VideoFilter + 'static) -> Self {
        self.filters
            .entry(track)
            .or_default()
            .push(Box::new(filter));
        self
    }

    /// Registers an audio decoder for a specific track.
    ///
    /// Audio tracks without a decoder will be passed through in copy mode.
    pub fn with_audio_decoder(
        mut self,
        track: TrackIndex,
        decoder: impl AudioDecoder + 'static,
    ) -> Self {
        self.audio_decoders.insert(track, Box::new(decoder));
        self
    }

    /// Registers an audio encoder for a specific track.
    ///
    /// Must be paired with an audio decoder for the same track.
    pub fn with_audio_encoder(
        mut self,
        track: TrackIndex,
        encoder: impl AudioEncoder + 'static,
    ) -> Self {
        self.audio_encoders.insert(track, Box::new(encoder));
        self
    }

    /// Registers an audio filter for a specific track.
    ///
    /// Filters are applied in order after decode and before encode.
    /// Multiple filters can be added per track by calling this multiple times.
    /// Only applies to audio tracks in transcode mode (with audio decoder + encoder).
    pub fn with_audio_filter(
        mut self,
        track: TrackIndex,
        filter: impl AudioFilter + 'static,
    ) -> Self {
        self.audio_filters
            .entry(track)
            .or_default()
            .push(Box::new(filter));
        self
    }

    /// Runs all pre-flight validation checks and returns every error found.
    ///
    /// Call this to get a complete list of configuration issues before
    /// committing to `build()`. The pipeline does not read any input data.
    ///
    /// `build()` calls `validate()` internally and fails on the first error,
    /// so callers using `build()` get validation for free.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if self.demuxer.is_none() {
            errors.push(ValidationError::MissingDemuxer);
        }
        if self.muxer.is_none() {
            errors.push(ValidationError::MissingMuxer);
        }

        // Video encoder without decoder
        for track in self.encoders.keys() {
            if !self.decoders.contains_key(track) {
                errors.push(ValidationError::EncoderWithoutDecoder(*track));
            }
        }
        // Video decoder without encoder
        for track in self.decoders.keys() {
            if !self.encoders.contains_key(track) {
                errors.push(ValidationError::DecoderWithoutEncoder(*track));
            }
        }
        // Audio encoder without decoder
        for track in self.audio_encoders.keys() {
            if !self.audio_decoders.contains_key(track) {
                errors.push(ValidationError::AudioEncoderWithoutDecoder(*track));
            }
        }
        // Audio decoder without encoder
        for track in self.audio_decoders.keys() {
            if !self.audio_encoders.contains_key(track) {
                errors.push(ValidationError::AudioDecoderWithoutEncoder(*track));
            }
        }
        // Orphan video filters (filter on a track without transcode chain)
        for track in self.filters.keys() {
            if !self.decoders.contains_key(track) || !self.encoders.contains_key(track) {
                errors.push(ValidationError::OrphanVideoFilter(*track));
            }
        }
        // Orphan audio filters
        for track in self.audio_filters.keys() {
            if !self.audio_decoders.contains_key(track) || !self.audio_encoders.contains_key(track)
            {
                errors.push(ValidationError::OrphanAudioFilter(*track));
            }
        }

        errors
    }

    /// Builds and returns a configured [`Pipeline`] ready to run.
    ///
    /// Calls [`validate()`](Self::validate) internally and returns the first
    /// error as a `PipelineError::Validation` if validation fails.
    pub fn build(mut self) -> Result<Pipeline<F>, PipelineError> {
        let errors = self.validate();
        if let Some(err) = errors.into_iter().next() {
            return Err(PipelineError::Validation(err));
        }

        let demuxer = self.demuxer.take().expect("validated: demuxer present");
        let muxer = self.muxer.take().expect("validated: muxer present");

        // Build track modes
        let mut track_modes: HashMap<TrackIndex, TrackMode> = HashMap::new();
        for (track, decoder) in self.decoders.drain() {
            let encoder = self.encoders.remove(&track).expect("validated above");
            let filters = self.filters.remove(&track).unwrap_or_default();
            track_modes.insert(
                track,
                TrackMode::Transcode {
                    decoder,
                    encoder,
                    filters,
                },
            );
        }
        for (track, decoder) in self.audio_decoders.drain() {
            let encoder = self.audio_encoders.remove(&track).expect("validated above");
            let filters = self.audio_filters.remove(&track).unwrap_or_default();
            track_modes.insert(
                track,
                TrackMode::AudioTranscode {
                    decoder,
                    encoder,
                    filters,
                },
            );
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

/// Drains all available frames from a decoder, applies filters, encodes them,
/// and writes resulting packets to the muxer.
fn drain_decoder_to_muxer<F: Fn(PipelineEvent)>(
    decoder: &mut dyn Decoder,
    encoder: &mut dyn Encoder,
    filters: &mut [Box<dyn VideoFilter>],
    muxer: &mut dyn Muxer,
    output_track: TrackIndex,
    on_event: &Option<F>,
    counters: &mut PipelineCounters,
) -> Result<(), PipelineError> {
    while let Some(frame) = decoder.receive_frame()? {
        counters.frames_decoded += 1;
        emit_event(
            on_event,
            PipelineEventKind::FramesDecoded {
                count: counters.frames_decoded,
            },
        );

        // Apply video filters if the frame is video
        let filtered_frame = if filters.is_empty() {
            frame
        } else {
            match frame {
                Frame::Video(vf) => {
                    let mut current = vf;
                    for filter in filters.iter_mut() {
                        current = filter.process(current)?;
                    }
                    Frame::Video(current)
                }
                other => other, // Audio frames pass through unmodified
            }
        };

        encoder.send_frame(Some(&filtered_frame))?;
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
        emit_event(
            on_event,
            PipelineEventKind::FramesEncoded {
                count: counters.frames_encoded,
            },
        );

        encoded_packet.track_index = output_track;
        muxer.write_packet(&encoded_packet)?;
        counters.packets_written += 1;
        emit_event(
            on_event,
            PipelineEventKind::PacketsWritten {
                count: counters.packets_written,
            },
        );
    }
    Ok(())
}

/// Drains all available frames from an audio decoder, applies audio filters,
/// encodes them, and writes resulting packets to the muxer.
fn drain_audio_decoder_to_muxer<F: Fn(PipelineEvent)>(
    decoder: &mut dyn AudioDecoder,
    encoder: &mut dyn AudioEncoder,
    filters: &mut [Box<dyn AudioFilter>],
    muxer: &mut dyn Muxer,
    output_track: TrackIndex,
    on_event: &Option<F>,
    counters: &mut PipelineCounters,
) -> Result<(), PipelineError> {
    while let Some(frame) = decoder.receive_frame()? {
        counters.frames_decoded += 1;
        emit_event(
            on_event,
            PipelineEventKind::FramesDecoded {
                count: counters.frames_decoded,
            },
        );

        // Apply audio filters
        let mut current = frame;
        for filter in filters.iter_mut() {
            current = filter.process(current)?;
        }

        encoder.send_frame(Some(&current))?;
        drain_audio_encoder_to_muxer(encoder, muxer, output_track, on_event, counters)?;
    }
    Ok(())
}

/// Drains all available packets from an audio encoder and writes them to the muxer.
fn drain_audio_encoder_to_muxer<F: Fn(PipelineEvent)>(
    encoder: &mut dyn AudioEncoder,
    muxer: &mut dyn Muxer,
    output_track: TrackIndex,
    on_event: &Option<F>,
    counters: &mut PipelineCounters,
) -> Result<(), PipelineError> {
    while let Some(mut encoded_packet) = encoder.receive_packet()? {
        counters.frames_encoded += 1;
        emit_event(
            on_event,
            PipelineEventKind::FramesEncoded {
                count: counters.frames_encoded,
            },
        );

        encoded_packet.track_index = output_track;
        muxer.write_packet(&encoded_packet)?;
        counters.packets_written += 1;
        emit_event(
            on_event,
            PipelineEventKind::PacketsWritten {
                count: counters.packets_written,
            },
        );
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
            emit_event(
                &self.on_event,
                PipelineEventKind::PacketsRead {
                    count: counters.packets_read,
                },
            );

            let input_track = packet.track_index;
            let output_track =
                input_to_output
                    .get(&input_track)
                    .copied()
                    .ok_or(PipelineError::Config {
                        message: format!(
                            "packet references track {} which was not in demuxer tracks",
                            input_track.0
                        ),
                    })?;

            match self.track_modes.get_mut(&input_track) {
                Some(TrackMode::Transcode {
                    decoder,
                    encoder,
                    filters,
                }) => {
                    decoder.send_packet(Some(&packet))?;
                    drain_decoder_to_muxer(
                        decoder.as_mut(),
                        encoder.as_mut(),
                        filters,
                        self.muxer.as_mut(),
                        output_track,
                        &self.on_event,
                        &mut counters,
                    )?;
                }
                Some(TrackMode::AudioTranscode {
                    decoder,
                    encoder,
                    filters,
                }) => {
                    decoder.send_packet(Some(&packet))?;
                    drain_audio_decoder_to_muxer(
                        decoder.as_mut(),
                        encoder.as_mut(),
                        filters,
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
                    emit_event(
                        &self.on_event,
                        PipelineEventKind::PacketsWritten {
                            count: counters.packets_written,
                        },
                    );
                }
            }
        }

        // Flush decoders and encoders at end-of-stream
        let track_indices: Vec<TrackIndex> = self.track_modes.keys().copied().collect();
        for track_idx in track_indices {
            let output_track = input_to_output[&track_idx];
            match self.track_modes.get_mut(&track_idx) {
                Some(TrackMode::Transcode {
                    decoder,
                    encoder,
                    filters,
                }) => {
                    // Signal end-of-stream to decoder, drain remaining frames
                    decoder.send_packet(None)?;
                    drain_decoder_to_muxer(
                        decoder.as_mut(),
                        encoder.as_mut(),
                        filters,
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
                Some(TrackMode::AudioTranscode {
                    decoder,
                    encoder,
                    filters,
                }) => {
                    decoder.send_packet(None)?;
                    drain_audio_decoder_to_muxer(
                        decoder.as_mut(),
                        encoder.as_mut(),
                        filters,
                        self.muxer.as_mut(),
                        output_track,
                        &self.on_event,
                        &mut counters,
                    )?;

                    encoder.send_frame(None)?;
                    drain_audio_encoder_to_muxer(
                        encoder.as_mut(),
                        self.muxer.as_mut(),
                        output_track,
                        &self.on_event,
                        &mut counters,
                    )?;
                }
                _ => {}
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
    use bytes::Bytes;
    use splica_core::{
        AudioCodec, AudioFrame, AudioTrackInfo, ChannelLayout, Codec, ColorSpace, DecodeError,
        DemuxError, EncodeError, FilterError, Frame, FrameRate, MuxError, Packet, PixelFormat,
        PlaneLayout, SampleFormat, Timestamp, TrackInfo, TrackKind, VideoCodec, VideoFrame,
        VideoTrackInfo,
    };
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
}
