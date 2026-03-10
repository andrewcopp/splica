//! Structured pipeline events for progress reporting and observability.
//!
//! Each event carries a monotonic timestamp and a typed payload describing
//! what happened. Designed for Prometheus-style scraping: counters are
//! cumulative, timestamps are monotonic, and the enum is non-exhaustive
//! so new event kinds can be added without breaking downstream code.

use std::time::Instant;

/// A structured event emitted by the pipeline during execution.
///
/// Events are designed for structured logging, progress bars, and metrics
/// collection. The `timestamp` field uses `std::time::Instant` for
/// monotonic, high-resolution timing suitable for rate calculations.
#[derive(Debug, Clone)]
pub struct PipelineEvent {
    /// When this event occurred (monotonic clock).
    pub timestamp: Instant,
    /// What happened.
    pub kind: PipelineEventKind,
}

impl PipelineEvent {
    /// Creates a new event with the current timestamp.
    pub fn new(kind: PipelineEventKind) -> Self {
        Self {
            timestamp: Instant::now(),
            kind,
        }
    }
}

/// The kind of pipeline event that occurred.
///
/// Each variant carries a cumulative `count` representing the total number
/// of items processed so far (not a delta). This makes metrics collection
/// simple: just read the latest count.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum PipelineEventKind {
    /// Compressed packets read from the demuxer.
    PacketsRead {
        /// Cumulative number of packets read.
        count: u64,
    },
    /// Frames decoded from compressed packets.
    FramesDecoded {
        /// Cumulative number of frames decoded.
        count: u64,
    },
    /// Frames encoded into compressed packets.
    FramesEncoded {
        /// Cumulative number of frames encoded.
        count: u64,
    },
    /// Compressed packets written to the muxer.
    PacketsWritten {
        /// Cumulative number of packets written.
        count: u64,
    },
    /// A non-fatal error occurred during pipeline execution.
    Error {
        /// Human-readable error message.
        message: String,
    },
}
