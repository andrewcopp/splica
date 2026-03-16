//! MP4 (ISO BMFF) demuxer and muxer.
//!
//! Currently implements read-only demuxing: parsing the MP4 container structure
//! and yielding compressed packets per track.

pub(crate) mod box_builders;
pub mod boxes;
pub(crate) mod codec_strings;
pub mod demuxer;
pub mod error;
pub(crate) mod fmp4_box_builders;
pub mod fmp4_muxer;
pub mod muxer;
pub mod sample_table;
pub(crate) mod track;

pub use demuxer::Mp4Demuxer;
pub use error::Mp4Error;
pub use fmp4_muxer::{FragmentConfig, FragmentedMp4Muxer};
pub use metadata::MetadataBox;
pub use muxer::Mp4Muxer;

pub mod metadata;

#[cfg(feature = "wasm")]
pub mod wasm;

#[cfg(test)]
mod test_helpers;

#[cfg(test)]
mod integration_tests;
