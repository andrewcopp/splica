//! Core pipeline traits: Demuxer, Decoder, Encoder, Muxer, Seekable, VideoFilter, AudioFilter.
//!
//! These traits define the five-stage media pipeline (demux → decode → filter → encode → mux)
//! plus seeking capability. I/O parameterization lives on the implementing struct, not the trait,
//! keeping traits object-safe.

use std::any::Any;

use crate::error::{DecodeError, DemuxError, EncodeError, FilterError, MuxError};
use crate::media::{AudioFrame, Frame, Packet, TrackIndex, TrackInfo, VideoFrame};
use crate::timestamp::Timestamp;

// ---------------------------------------------------------------------------
// Demuxer
// ---------------------------------------------------------------------------

/// Reads a container format and yields compressed packets per stream.
///
/// Implementations are generic over their I/O source (e.g., `Mp4Demuxer<R: Read + Seek>`),
/// but this trait is I/O-agnostic and object-safe.
pub trait Demuxer {
    /// Returns metadata for all tracks in the container.
    fn tracks(&self) -> &[TrackInfo];

    /// Reads the next packet from the container.
    ///
    /// Returns `Ok(None)` at end of stream.
    fn read_packet(&mut self) -> Result<Option<Packet>, DemuxError>;
}

impl Demuxer for Box<dyn Demuxer> {
    fn tracks(&self) -> &[TrackInfo] {
        (**self).tracks()
    }

    fn read_packet(&mut self) -> Result<Option<Packet>, DemuxError> {
        (**self).read_packet()
    }
}

// ---------------------------------------------------------------------------
// Seekable
// ---------------------------------------------------------------------------

/// How to seek within a media stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeekMode {
    /// Seek to the nearest keyframe at or before the target timestamp.
    /// Fast, but the next decoded frame may not be exactly at `target`.
    Keyframe,
    /// Seek to the exact frame at `target`. May require decoding forward
    /// from the previous keyframe. Slower, but frame-accurate.
    Precise,
}

/// Optional seeking capability for demuxers.
///
/// Not all sources support seeking (e.g., streaming over HTTP without
/// range requests). Implementations that support seeking implement both
/// `Demuxer` and `Seekable`.
///
/// # Usage
///
/// ```ignore
/// fn seek_and_read<D: Demuxer + Seekable>(demuxer: &mut D) -> Result<(), DemuxError> {
///     demuxer.seek(Timestamp::new(150, 30), SeekMode::Keyframe)?;
///     while let Some(packet) = demuxer.read_packet()? {
///         // process packets from new position
///     }
///     Ok(())
/// }
/// ```
pub trait Seekable {
    /// Seeks to the given timestamp using the specified mode.
    fn seek(&mut self, target: Timestamp, mode: SeekMode) -> Result<(), DemuxError>;
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

/// Decompresses packets into raw frames.
///
/// Uses the send/receive pattern to handle codecs that buffer multiple packets
/// before producing output (e.g., B-frame reordering, codec lookahead).
///
/// # Codec-specific parameters
///
/// The `Decoder` trait abstracts away codec details for generic pipeline use.
/// When you need codec-specific access (e.g., H.264 profile/level, reference
/// frame count), downcast via [`as_any()`](Decoder::as_any):
///
/// ```ignore
/// use splica_codec::H264Decoder;
///
/// fn inspect_decoder(decoder: &dyn Decoder) {
///     if let Some(h264) = decoder.as_any().downcast_ref::<H264Decoder>() {
///         let config = h264.codec_config();
///         println!("H.264 profile: {:?}, level: {:?}", config.profile, config.level);
///     }
/// }
/// ```
///
/// This pattern preserves object safety while giving power users (like Marcus
/// for video editing or Elena for broadcast compliance) access to codec internals.
pub trait Decoder {
    /// Sends a compressed packet to the decoder.
    ///
    /// Pass `None` to signal end-of-stream and flush any buffered frames.
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError>;

    /// Receives a decoded frame from the decoder.
    ///
    /// Returns `Ok(None)` when no more frames are available (call `send_packet` again).
    fn receive_frame(&mut self) -> Result<Option<Frame>, DecodeError>;

    /// Returns a reference to the concrete decoder type for downcasting.
    ///
    /// Used to access codec-specific parameters that the generic `Decoder`
    /// trait intentionally doesn't expose.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to the concrete decoder type for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ---------------------------------------------------------------------------
// AudioDecoder
// ---------------------------------------------------------------------------

/// Decompresses audio packets into raw audio frames.
///
/// Uses the same send/receive pattern as [`Decoder`] for consistency.
/// Audio codecs may buffer multiple packets before producing output
/// (e.g., SBR in AAC, Opus lookahead).
///
/// # Codec-specific parameters
///
/// Use [`as_any()`](AudioDecoder::as_any) to downcast to the concrete type
/// for codec-specific access (e.g., AAC profile, Opus bandwidth mode).
pub trait AudioDecoder {
    /// Sends a compressed audio packet to the decoder.
    ///
    /// Pass `None` to signal end-of-stream and flush any buffered frames.
    fn send_packet(&mut self, packet: Option<&Packet>) -> Result<(), DecodeError>;

    /// Receives a decoded audio frame from the decoder.
    ///
    /// Returns `Ok(None)` when no more frames are available (call `send_packet` again).
    fn receive_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError>;

    /// Returns a reference to the concrete decoder type for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to the concrete decoder type for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ---------------------------------------------------------------------------
// AudioEncoder
// ---------------------------------------------------------------------------

/// Compresses raw audio frames into packets.
///
/// Uses the same send/receive pattern as [`Encoder`] for consistency.
///
/// Codec-specific configuration (bitrate, sample rate, etc.) is set on the
/// concrete encoder type at construction time. Use [`as_any()`](AudioEncoder::as_any)
/// to downcast and inspect codec-specific state at runtime.
pub trait AudioEncoder {
    /// Sends a raw audio frame to the encoder.
    ///
    /// Pass `None` to signal end-of-stream and flush any buffered packets.
    fn send_frame(&mut self, frame: Option<&AudioFrame>) -> Result<(), EncodeError>;

    /// Receives a compressed packet from the encoder.
    ///
    /// Returns `Ok(None)` when no more packets are available (call `send_frame` again).
    fn receive_packet(&mut self) -> Result<Option<Packet>, EncodeError>;

    /// Returns a reference to the concrete encoder type for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to the concrete encoder type for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

/// Compresses raw frames into packets.
///
/// Uses the send/receive pattern to handle encoders with lookahead buffers.
///
/// Codec-specific configuration (bitrate, profile, etc.) is set on the concrete
/// encoder type at construction time. Use [`as_any()`](Encoder::as_any) to
/// downcast and inspect codec-specific state at runtime.
pub trait Encoder {
    /// Sends a raw frame to the encoder.
    ///
    /// Pass `None` to signal end-of-stream and flush any buffered packets.
    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<(), EncodeError>;

    /// Receives a compressed packet from the encoder.
    ///
    /// Returns `Ok(None)` when no more packets are available (call `send_frame` again).
    fn receive_packet(&mut self) -> Result<Option<Packet>, EncodeError>;

    /// Returns a reference to the concrete encoder type for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to the concrete encoder type for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ---------------------------------------------------------------------------
// Muxer
// ---------------------------------------------------------------------------

/// Writes compressed packets into a container format.
pub trait Muxer {
    /// Registers a track and returns its index in the output container.
    fn add_track(&mut self, info: &TrackInfo) -> Result<TrackIndex, MuxError>;

    /// Writes a compressed packet to the container.
    fn write_packet(&mut self, packet: &Packet) -> Result<(), MuxError>;

    /// Finalizes the container (writes headers, indices, etc.) and flushes all data.
    fn finalize(&mut self) -> Result<(), MuxError>;
}

impl Muxer for Box<dyn Muxer> {
    fn add_track(&mut self, info: &TrackInfo) -> Result<TrackIndex, MuxError> {
        (**self).add_track(info)
    }

    fn write_packet(&mut self, packet: &Packet) -> Result<(), MuxError> {
        (**self).write_packet(packet)
    }

    fn finalize(&mut self) -> Result<(), MuxError> {
        (**self).finalize()
    }
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

/// A video frame transform (scale, crop, color convert, etc.).
///
/// Filters operate on concrete `VideoFrame` values — they never see the
/// `Frame` enum. Pipeline-level dispatch is handled by the pipeline crate.
pub trait VideoFilter {
    /// Processes a single video frame.
    fn process(&mut self, frame: VideoFrame) -> Result<VideoFrame, FilterError>;

    /// Flushes any internally buffered frames.
    ///
    /// Called at end-of-stream. The default implementation returns an empty vec
    /// (appropriate for stateless filters like crop or scale).
    fn flush(&mut self) -> Result<Vec<VideoFrame>, FilterError> {
        Ok(vec![])
    }
}

/// An audio frame transform (resample, mix, volume, etc.).
///
/// Filters operate on concrete `AudioFrame` values — they never see the
/// `Frame` enum. Pipeline-level dispatch is handled by the pipeline crate.
pub trait AudioFilter {
    /// Processes a single audio frame.
    fn process(&mut self, frame: AudioFrame) -> Result<AudioFrame, FilterError>;

    /// Flushes any internally buffered frames.
    ///
    /// Called at end-of-stream. The default implementation returns an empty vec
    /// (appropriate for stateless filters like volume adjustment).
    fn flush(&mut self) -> Result<Vec<AudioFrame>, FilterError> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::*;
    use bytes::Bytes;
    use std::io::Cursor;

    // A minimal in-memory demuxer to prove Marcus can implement the trait
    // and Alex can use it with Cursor<Vec<u8>>.
    struct TestDemuxer {
        tracks: Vec<TrackInfo>,
        packets: Vec<Packet>,
        position: usize,
    }

    impl TestDemuxer {
        fn new(_reader: Cursor<Vec<u8>>, tracks: Vec<TrackInfo>, packets: Vec<Packet>) -> Self {
            Self {
                tracks,
                packets,
                position: 0,
            }
        }
    }

    impl Demuxer for TestDemuxer {
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

    impl Seekable for TestDemuxer {
        fn seek(&mut self, _target: Timestamp, _mode: SeekMode) -> Result<(), DemuxError> {
            self.position = 0;
            Ok(())
        }
    }

    #[test]
    fn test_that_custom_demuxer_works_with_cursor() {
        // GIVEN — Marcus implements Demuxer for his format, Alex uses Cursor
        let data = Cursor::new(vec![0u8; 100]);
        let track = TrackInfo {
            index: TrackIndex(0),
            kind: TrackKind::Video,
            codec: Codec::Video(VideoCodec::H264),
            duration: None,
            video: Some(VideoTrackInfo {
                width: 1920,
                height: 1080,
                pixel_format: Some(PixelFormat::Yuv420p),
                color_space: Some(ColorSpace::BT709),
                frame_rate: FrameRate::new(30, 1),
            }),
            audio: None,
        };
        let packet = Packet {
            track_index: TrackIndex(0),
            pts: Timestamp::new(0, 30).unwrap(),
            dts: Timestamp::new(0, 30).unwrap(),
            is_keyframe: true,
            data: Bytes::from_static(b"fake h264 data"),
        };
        let mut demuxer = TestDemuxer::new(data, vec![track], vec![packet]);

        // WHEN / THEN
        assert_eq!(demuxer.tracks().len(), 1);
        assert!(demuxer.read_packet().unwrap().is_some());
        assert!(demuxer.read_packet().unwrap().is_none());
    }

    #[test]
    fn test_that_demuxer_is_object_safe() {
        // GIVEN — Priya uses dyn Demuxer in her pipeline orchestration
        let data = Cursor::new(vec![0u8; 100]);
        let mut demuxer = TestDemuxer::new(data, vec![], vec![]);

        // WHEN — used as a trait object
        let dyn_demuxer: &mut dyn Demuxer = &mut demuxer;

        // THEN — trait object works
        assert_eq!(dyn_demuxer.tracks().len(), 0);
        assert!(dyn_demuxer.read_packet().unwrap().is_none());
    }

    #[test]
    fn test_that_seekable_works_with_demuxer() {
        // GIVEN
        let data = Cursor::new(vec![0u8; 100]);
        let packet = Packet {
            track_index: TrackIndex(0),
            pts: Timestamp::new(0, 30).unwrap(),
            dts: Timestamp::new(0, 30).unwrap(),
            is_keyframe: true,
            data: Bytes::from_static(b"data"),
        };
        let mut demuxer = TestDemuxer::new(data, vec![], vec![packet]);

        // Consume the packet
        let _ = demuxer.read_packet().unwrap();
        assert!(demuxer.read_packet().unwrap().is_none());

        // WHEN — seek back
        demuxer
            .seek(Timestamp::new(0, 30).unwrap(), SeekMode::Keyframe)
            .unwrap();

        // THEN — can read again
        assert!(demuxer.read_packet().unwrap().is_some());
    }

    #[test]
    fn test_that_error_kind_is_matchable_on_demux_error() {
        // GIVEN — Priya matches on error kind for retry decisions
        let error = DemuxError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset",
        ));

        // WHEN
        let kind = error.kind();

        // THEN
        assert!(kind.is_retryable());
    }
}
