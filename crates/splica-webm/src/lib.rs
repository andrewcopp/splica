//! WebM (Matroska subset) demuxer and muxer.

pub mod ebml;
pub mod elements;
pub mod error;

mod demuxer;

pub use demuxer::WebmDemuxer;
pub use error::WebmError;
