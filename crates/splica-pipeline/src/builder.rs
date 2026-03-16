//! Pipeline builder — configures demuxer, muxer, codecs, and filters.

use std::collections::HashMap;

use splica_core::{
    AudioDecoder, AudioEncoder, AudioFilter, Codec, Decoder, Demuxer, Encoder, Muxer,
    PipelineError, TrackIndex, ValidationError, VideoFilter,
};

use crate::event::PipelineEvent;
use crate::pipeline::Pipeline;

/// How the pipeline should handle a track.
pub(crate) enum TrackMode {
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
#[must_use]
pub struct PipelineBuilder {
    pub(crate) on_event: Option<Box<dyn Fn(PipelineEvent)>>,
    pub(crate) demuxer: Option<Box<dyn Demuxer>>,
    pub(crate) muxer: Option<Box<dyn Muxer>>,
    pub(crate) decoders: HashMap<TrackIndex, Box<dyn Decoder>>,
    pub(crate) encoders: HashMap<TrackIndex, Box<dyn Encoder>>,
    pub(crate) filters: HashMap<TrackIndex, Vec<Box<dyn VideoFilter>>>,
    pub(crate) audio_decoders: HashMap<TrackIndex, Box<dyn AudioDecoder>>,
    pub(crate) audio_encoders: HashMap<TrackIndex, Box<dyn AudioEncoder>>,
    pub(crate) audio_filters: HashMap<TrackIndex, Vec<Box<dyn AudioFilter>>>,
    pub(crate) output_codecs: HashMap<TrackIndex, Codec>,
    pub(crate) output_dimensions: HashMap<TrackIndex, (u32, u32)>,
    pub(crate) max_fps: HashMap<TrackIndex, f32>,
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineBuilder {
    /// Creates a new pipeline builder with no components configured.
    ///
    /// The builder starts empty. At minimum, you must provide a demuxer
    /// (via [`with_demuxer()`](Self::with_demuxer)) and a muxer (via
    /// [`with_muxer()`](Self::with_muxer)) before calling
    /// [`build()`](Self::build). Tracks without explicit decoder/encoder
    /// pairs default to copy mode (compressed packets pass through
    /// untouched).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use splica_pipeline::PipelineBuilder;
    ///
    /// // Copy mode: remux without transcoding
    /// let mut pipeline = PipelineBuilder::new()
    ///     .with_demuxer(my_demuxer)
    ///     .with_muxer(my_muxer)
    ///     .build()?;
    /// pipeline.run()?;
    /// ```
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
            output_codecs: HashMap::new(),
            output_dimensions: HashMap::new(),
            max_fps: HashMap::new(),
        }
    }

    /// Sets a callback that receives pipeline events for progress reporting.
    ///
    /// The handler is called synchronously on the pipeline's thread as each
    /// event occurs. Keep the callback fast to avoid stalling the pipeline.
    /// For heavy work (e.g., database writes), send events to a channel and
    /// process them on a separate thread.
    ///
    /// # Examples
    ///
    /// ```
    /// use splica_pipeline::{PipelineBuilder, PipelineEvent, PipelineEventKind};
    /// use std::sync::{Arc, Mutex};
    ///
    /// let packets_read = Arc::new(Mutex::new(0u64));
    /// let counter = Arc::clone(&packets_read);
    ///
    /// let builder = PipelineBuilder::new()
    ///     .with_event_handler(move |event: PipelineEvent| {
    ///         if let PipelineEventKind::PacketsRead { count } = event.kind {
    ///             *counter.lock().unwrap() = count;
    ///         }
    ///     });
    /// ```
    pub fn with_event_handler(mut self, handler: impl Fn(PipelineEvent) + 'static) -> Self {
        self.on_event = Some(Box::new(handler));
        self
    }

    /// Sets the demuxer (input source).
    ///
    /// Accepts any type that implements [`Demuxer`], including custom
    /// implementations. The demuxer is consumed and stored as a trait
    /// object, so you can swap container formats (MP4, WebM, MKV) or
    /// use in-memory sources without changing the pipeline setup.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use splica_mp4::Mp4Demuxer;
    /// use std::fs::File;
    /// use std::io::BufReader;
    ///
    /// // File-based MP4 input
    /// let file = BufReader::new(File::open("input.mp4")?);
    /// let demuxer = Mp4Demuxer::open(file)?;
    ///
    /// let builder = PipelineBuilder::new().with_demuxer(demuxer);
    /// ```
    ///
    /// ```ignore
    /// use std::io::Cursor;
    ///
    /// // In-memory input with a custom demuxer
    /// let data = Cursor::new(raw_bytes);
    /// let demuxer = MyCustomDemuxer::new(data);
    ///
    /// let builder = PipelineBuilder::new().with_demuxer(demuxer);
    /// ```
    pub fn with_demuxer(mut self, demuxer: impl Demuxer + 'static) -> Self {
        self.demuxer = Some(Box::new(demuxer));
        self
    }

    /// Sets the muxer (output destination).
    ///
    /// Accepts any type that implements [`Muxer`]. Like the demuxer, the
    /// muxer is stored as a trait object so you can target different
    /// container formats or write to in-memory buffers.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use splica_mp4::Mp4Muxer;
    /// use std::fs::File;
    /// use std::io::BufWriter;
    ///
    /// let file = BufWriter::new(File::create("output.mp4")?);
    /// let muxer = Mp4Muxer::new(file);
    ///
    /// let builder = PipelineBuilder::new().with_muxer(muxer);
    /// ```
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
    ///
    /// # Filter chain composition
    ///
    /// Filters execute in the order they are added. Each filter receives the
    /// output of the previous one, forming a pipeline within the pipeline:
    ///
    /// ```ignore
    /// use splica_core::TrackIndex;
    /// use splica_filter::{ScaleFilter, CropFilter};
    ///
    /// let builder = PipelineBuilder::new()
    ///     .with_demuxer(demuxer)
    ///     .with_muxer(muxer)
    ///     .with_decoder(TrackIndex(0), decoder)
    ///     .with_encoder(TrackIndex(0), encoder)
    ///     // Filters run left-to-right: scale first, then crop
    ///     .with_filter(TrackIndex(0), ScaleFilter::new(1280, 720))
    ///     .with_filter(TrackIndex(0), CropFilter::new(100, 50, 1080, 620));
    /// ```
    ///
    /// # Errors
    ///
    /// Adding a filter to a track without a decoder/encoder pair will cause
    /// [`validate()`](Self::validate) (and therefore [`build()`](Self::build))
    /// to return [`ValidationError::OrphanVideoFilter`].
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

    /// Overrides the codec reported to the muxer for a specific track.
    ///
    /// When an encoder produces a different codec than the input (e.g., H.264
    /// input transcoded to H.265), the muxer needs to know the output codec
    /// to write the correct sample description. This override is applied to
    /// the track info before calling `Muxer::add_track`.
    pub fn with_output_codec(mut self, track: TrackIndex, codec: Codec) -> Self {
        self.output_codecs.insert(track, codec);
        self
    }

    /// Overrides the video dimensions reported to the muxer for a specific track.
    ///
    /// When a resize filter changes the frame dimensions, the muxer needs to
    /// know the output dimensions to write correct container metadata (e.g.,
    /// tkhd/stsd in MP4). This override is applied to the track info before
    /// calling `Muxer::add_track`.
    pub fn with_output_dimensions(mut self, track: TrackIndex, width: u32, height: u32) -> Self {
        self.output_dimensions.insert(track, (width, height));
        self
    }

    /// Sets a maximum output frame rate for a video track.
    ///
    /// Frames whose PTS is less than `1/fps` seconds after the previously
    /// emitted frame are dropped before filtering and encoding.
    pub fn with_max_fps(mut self, track: TrackIndex, fps: f32) -> Self {
        self.max_fps.insert(track, fps);
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
    /// error as a [`PipelineError::Validation`] if validation fails.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use splica_core::TrackIndex;
    /// use splica_pipeline::PipelineBuilder;
    ///
    /// // Full transcode pipeline: demux → decode → filter → encode → mux
    /// let mut pipeline = PipelineBuilder::new()
    ///     .with_demuxer(demuxer)
    ///     .with_decoder(TrackIndex(0), h264_decoder)
    ///     .with_filter(TrackIndex(0), scale_filter)
    ///     .with_encoder(TrackIndex(0), h265_encoder)
    ///     .with_output_codec(TrackIndex(0), Codec::Video(VideoCodec::H265))
    ///     .with_muxer(mp4_muxer)
    ///     .build()?;
    ///
    /// pipeline.run()?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::Validation`] if:
    /// - No demuxer is configured
    /// - No muxer is configured
    /// - An encoder exists without a matching decoder (or vice versa)
    /// - A filter exists on a track without a decoder/encoder pair
    pub fn build(mut self) -> Result<Pipeline, PipelineError> {
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
            output_codecs: self.output_codecs,
            output_dimensions: self.output_dimensions,
            on_event: self.on_event,
            max_fps: self.max_fps,
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
