//! Shared types, traits, and error types for the splica media processing library.

pub mod error;
pub mod media;
pub mod smpte;
pub mod timestamp;
pub mod traits;
#[cfg(feature = "serde")]
pub mod wasm_types;

pub use error::{
    DecodeError, DemuxError, EncodeError, ErrorKind, FilterError, MuxError, PipelineError,
};
pub use media::{
    AudioCodec, AudioFrame, AudioTrackInfo, ChannelLayout, Codec, ColorPrimaries, ColorRange,
    ColorSpace, ContainerFormat, Frame, FrameRate, MatrixCoefficients, Packet, PixelFormat,
    PlaneLayout, QualityTarget, ResourceBudget, SampleFormat, TrackIndex, TrackInfo, TrackKind,
    TransferCharacteristics, VideoCodec, VideoFrame, VideoFrameError, VideoTrackInfo,
};
pub use smpte::SmpteTimecode;
pub use timestamp::Timestamp;
pub use traits::{
    AudioDecoder, AudioEncoder, AudioFilter, Decoder, Demuxer, Encoder, Muxer, SeekMode, Seekable,
    VideoFilter,
};
