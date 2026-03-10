//! MP4 (ISO BMFF) demuxer and muxer.
//!
//! Currently implements read-only demuxing: parsing the MP4 container structure
//! and yielding compressed packets per track.

pub mod boxes;
pub mod demuxer;
pub mod error;
pub mod muxer;
pub mod sample_table;
pub(crate) mod track;

pub use demuxer::Mp4Demuxer;
pub use error::Mp4Error;
pub use muxer::Mp4Muxer;

#[cfg(feature = "wasm")]
pub mod wasm;

#[cfg(test)]
mod test_helpers;

#[cfg(test)]
mod integration_tests;
