//! High-level pipeline orchestration connecting demux, decode, filter, encode, and mux.
//!
//! The pipeline drives the canonical media processing loop:
//! demux → decode → encode → mux, with optional event callbacks for progress
//! reporting. Tracks without a configured decoder/encoder are passed through
//! in copy mode (compressed packets go directly from demuxer to muxer).

mod builder;
pub mod event;
mod pipeline;

pub use builder::PipelineBuilder;
pub use event::{PipelineEvent, PipelineEventKind};
pub use pipeline::Pipeline;

#[cfg(test)]
mod tests;
