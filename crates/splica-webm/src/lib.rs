//! WebM (Matroska subset) demuxer and muxer.

pub mod ebml;
pub mod elements;
pub mod error;

mod demuxer;
mod muxer;

pub use demuxer::WebmDemuxer;
pub use error::WebmError;
pub use muxer::WebmMuxer;
