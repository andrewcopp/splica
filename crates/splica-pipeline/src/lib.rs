//! High-level pipeline orchestration connecting demux, decode, filter, encode, and mux.
//!
//! The pipeline drives the canonical media processing loop:
//! demux → decode → encode → mux, with optional event callbacks for progress
//! reporting. Tracks without a configured decoder/encoder are passed through
//! in copy mode (compressed packets go directly from demuxer to muxer).
//!
//! # In-memory processing
//!
//! All demuxers and muxers are generic over `Read + Seek` / `Write + Seek`,
//! so you can process video entirely in memory using [`std::io::Cursor`]:
//!
//! ```no_run
//! use std::io::Cursor;
//! use splica_mp4::{Mp4Demuxer, Mp4Muxer};
//! use splica_pipeline::PipelineBuilder;
//!
//! // Read input from an in-memory buffer (e.g., from a network response)
//! let input_bytes: Vec<u8> = std::fs::read("input.mp4").unwrap();
//! let demuxer = Mp4Demuxer::open(Cursor::new(input_bytes)).unwrap();
//!
//! // Write output to an in-memory buffer
//! let output_buf = Cursor::new(Vec::new());
//! let muxer = Mp4Muxer::new(output_buf);
//!
//! // Build and run a stream-copy pipeline (no codecs needed)
//! let mut pipeline = PipelineBuilder::new()
//!     .with_demuxer(demuxer)
//!     .with_muxer(muxer)
//!     .build()
//!     .unwrap();
//!
//! pipeline.run().unwrap();
//! ```
//!
//! This pattern works on any platform including iOS, Android, and WASM.
//! For WebM or MKV containers, substitute [`splica_webm::WebmDemuxer`] or
//! [`splica_mkv::MkvDemuxer`] respectively.

mod builder;
pub mod event;
mod pipeline;

pub use builder::PipelineBuilder;
pub use event::{PipelineEvent, PipelineEventKind};
pub use pipeline::Pipeline;

#[cfg(test)]
mod tests;
