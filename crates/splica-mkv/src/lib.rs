//! MKV (Matroska) demuxer and muxer.
//!
//! Reads and writes compressed packets in a Matroska container. Uses the same
//! EBML encoding as WebM but with the `"matroska"` document type, supporting a
//! broader set of codecs (H.264, H.265, VP9, AV1, AAC, Opus, etc.).

mod demuxer;
mod ebml;
mod elements;
pub mod error;
mod muxer;

pub use demuxer::MkvDemuxer;
pub use error::MkvError;
pub use muxer::MkvMuxer;
