//! Pipeline execution — the demux → decode → filter → encode → mux loop.

use std::collections::HashMap;

use splica_core::{
    AudioDecoder, AudioEncoder, AudioFilter, Codec, Decoder, Demuxer, Encoder, Frame, Muxer,
    PipelineError, TrackIndex, VideoFilter,
};

use crate::builder::TrackMode;
use crate::event::{PipelineEvent, PipelineEventKind};

/// A configured media processing pipeline ready to execute.
///
/// Created via [`PipelineBuilder::build()`](crate::PipelineBuilder::build).
/// Call [`run()`](Pipeline::run) to process all packets from the demuxer
/// through to the muxer.
pub struct Pipeline {
    pub(crate) demuxer: Box<dyn Demuxer>,
    pub(crate) muxer: Box<dyn Muxer>,
    pub(crate) track_modes: HashMap<TrackIndex, TrackMode>,
    pub(crate) output_codecs: HashMap<TrackIndex, Codec>,
    pub(crate) output_dimensions: HashMap<TrackIndex, (u32, u32)>,
    pub(crate) on_event: Option<Box<dyn Fn(PipelineEvent)>>,
    /// Maximum output frame rate per track. Frames closer together than
    /// 1/max_fps seconds are dropped before encoding.
    pub(crate) max_fps: HashMap<TrackIndex, f32>,
}

fn emit_event(on_event: &Option<Box<dyn Fn(PipelineEvent)>>, kind: PipelineEventKind) {
    if let Some(ref f) = on_event {
        f(PipelineEvent::new(kind));
    }
}

/// Per-track frame rate limiting state.
struct FrameRateLimit {
    /// Minimum interval between emitted frames in seconds.
    min_interval: f64,
    /// PTS of the last emitted frame.
    last_emitted_pts: Option<f64>,
}

impl FrameRateLimit {
    fn new(max_fps: f32) -> Self {
        Self {
            min_interval: 1.0 / max_fps as f64,
            last_emitted_pts: None,
        }
    }

    /// Returns `true` if the frame should be dropped.
    fn should_drop(&mut self, pts_secs: f64) -> bool {
        if let Some(last) = self.last_emitted_pts {
            if pts_secs - last < self.min_interval {
                return true;
            }
        }
        self.last_emitted_pts = Some(pts_secs);
        false
    }
}

/// Drains all available frames from a decoder, applies filters, encodes them,
/// and writes resulting packets to the muxer.
#[allow(clippy::too_many_arguments)]
fn drain_decoder_to_muxer(
    decoder: &mut dyn Decoder,
    encoder: &mut dyn Encoder,
    filters: &mut [Box<dyn VideoFilter>],
    muxer: &mut dyn Muxer,
    output_track: TrackIndex,
    on_event: &Option<Box<dyn Fn(PipelineEvent)>>,
    counters: &mut PipelineCounters,
    mut rate_limit: Option<&mut FrameRateLimit>,
) -> Result<(), PipelineError> {

    while let Some(frame) = decoder.receive_frame()? {
        counters.frames_decoded += 1;
        emit_event(
            on_event,
            PipelineEventKind::FramesDecoded {
                count: counters.frames_decoded,
            },
        );

        // Drop frames that exceed the max frame rate
        if let Some(ref mut limiter) = rate_limit {
            let pts_secs = match &frame {
                Frame::Video(vf) => vf.pts.as_seconds_f64(),
                Frame::Audio(af) => af.pts.as_seconds_f64(),
            };
            if limiter.should_drop(pts_secs) {
                continue;
            }
        }

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
fn drain_encoder_to_muxer(
    encoder: &mut dyn Encoder,
    muxer: &mut dyn Muxer,
    output_track: TrackIndex,
    on_event: &Option<Box<dyn Fn(PipelineEvent)>>,
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
fn drain_audio_decoder_to_muxer(
    decoder: &mut dyn AudioDecoder,
    encoder: &mut dyn AudioEncoder,
    filters: &mut [Box<dyn AudioFilter>],
    muxer: &mut dyn Muxer,
    output_track: TrackIndex,
    on_event: &Option<Box<dyn Fn(PipelineEvent)>>,
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
fn drain_audio_encoder_to_muxer(
    encoder: &mut dyn AudioEncoder,
    muxer: &mut dyn Muxer,
    output_track: TrackIndex,
    on_event: &Option<Box<dyn Fn(PipelineEvent)>>,
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

impl Pipeline {
    /// Runs the pipeline to completion.
    ///
    /// Processes all packets from the demuxer, routing each through the
    /// appropriate decode→encode path or copy mode, then writes to the muxer.
    /// Flushes decoders and encoders at end-of-stream and finalizes the muxer.
    pub fn run(&mut self) -> Result<(), PipelineError> {
        // Register all input tracks with the muxer, applying codec overrides
        let tracks = self.demuxer.tracks().to_vec();
        let mut input_to_output: HashMap<TrackIndex, TrackIndex> = HashMap::new();
        for track_info in &tracks {
            let mut info = if let Some(codec) = self.output_codecs.get(&track_info.index) {
                let mut overridden = track_info.clone();
                overridden.codec = codec.clone();
                overridden
            } else {
                track_info.clone()
            };
            if let Some(&(w, h)) = self.output_dimensions.get(&track_info.index) {
                if let Some(ref mut video) = info.video {
                    video.width = w;
                    video.height = h;
                }
            }
            let output_idx = self.muxer.add_track(&info)?;
            input_to_output.insert(track_info.index, output_idx);
        }

        let mut counters = PipelineCounters {
            packets_read: 0,
            frames_decoded: 0,
            frames_encoded: 0,
            packets_written: 0,
        };

        // Per-track frame rate limiting state
        let mut rate_limits: HashMap<TrackIndex, FrameRateLimit> = self
            .max_fps
            .iter()
            .map(|(&track, &fps)| (track, FrameRateLimit::new(fps)))
            .collect();

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
                        rate_limits.get_mut(&input_track),
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
                None => {
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
                        rate_limits.get_mut(&track_idx),
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
mod frame_rate_limit_tests {
    use super::FrameRateLimit;

    #[test]
    fn test_that_first_frame_is_never_dropped() {
        let mut limiter = FrameRateLimit::new(30.0);

        assert!(!limiter.should_drop(0.0));
    }

    #[test]
    fn test_that_frame_within_interval_is_dropped() {
        let mut limiter = FrameRateLimit::new(30.0); // 33.3ms interval

        assert!(!limiter.should_drop(0.0));
        assert!(limiter.should_drop(0.010)); // 10ms < 33.3ms
        assert!(limiter.should_drop(0.020)); // 20ms < 33.3ms
        assert!(limiter.should_drop(0.030)); // 30ms < 33.3ms
    }

    #[test]
    fn test_that_frame_past_interval_is_emitted() {
        let mut limiter = FrameRateLimit::new(30.0); // 33.3ms interval

        assert!(!limiter.should_drop(0.0));
        assert!(!limiter.should_drop(0.034)); // 34ms > 33.3ms
    }

    #[test]
    fn test_that_interval_resets_after_emission() {
        let mut limiter = FrameRateLimit::new(10.0); // 100ms interval

        assert!(!limiter.should_drop(0.0));
        assert!(limiter.should_drop(0.050));  // 50ms < 100ms, dropped
        assert!(!limiter.should_drop(0.100)); // 100ms >= 100ms, emitted
        assert!(limiter.should_drop(0.150));  // 50ms since last emit, dropped
        assert!(!limiter.should_drop(0.200)); // 100ms since last emit, emitted
    }

    #[test]
    fn test_that_max_fps_one_allows_one_frame_per_second() {
        let mut limiter = FrameRateLimit::new(1.0); // 1000ms interval

        assert!(!limiter.should_drop(0.0));
        assert!(limiter.should_drop(0.5));
        assert!(limiter.should_drop(0.999));
        assert!(!limiter.should_drop(1.0));
    }
}
