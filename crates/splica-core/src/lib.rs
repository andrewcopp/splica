//! Shared types, traits, and error types for the splica media processing library.

pub mod error;
pub mod media;
pub mod smpte;
pub mod timestamp;
pub mod traits;

pub use error::{
    DecodeError, DemuxError, EncodeError, ErrorKind, FilterError, MuxError, PipelineError,
};
pub use media::{
    AudioCodec, AudioFrame, AudioTrackInfo, ChannelLayout, Codec, ColorPrimaries, ColorRange,
    ColorSpace, Frame, FrameRate, MatrixCoefficients, Packet, PixelFormat, PlaneLayout,
    ResourceBudget, SampleFormat, TrackIndex, TrackInfo, TrackKind, TransferCharacteristics,
    VideoCodec, VideoFrame, VideoFrameError, VideoTrackInfo,
};
pub use smpte::SmpteTimecode;
pub use timestamp::Timestamp;
pub use traits::{AudioFilter, Decoder, Demuxer, Encoder, Muxer, SeekMode, Seekable, VideoFilter};
