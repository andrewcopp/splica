//! Shared types, traits, and error types for the splica media processing library.

pub mod error;
pub mod media;
pub mod timestamp;
pub mod traits;

pub use error::{
    DecodeError, DemuxError, EncodeError, ErrorKind, FilterError, MuxError, PipelineError,
};
pub use media::{
    AudioCodec, AudioFrame, AudioTrackInfo, ChannelLayout, Codec, ColorPrimaries, ColorSpace,
    Frame, MatrixCoefficients, Packet, PixelFormat, SampleFormat, TrackIndex, TrackInfo, TrackKind,
    TransferCharacteristics, VideoCodec, VideoFrame, VideoTrackInfo,
};
pub use timestamp::Timestamp;
pub use traits::{
    AudioFilter, Decoder, Demuxer, Encoder, Muxer, SeekMode, Seekable, VideoFilter,
};
