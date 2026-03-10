//! WebM/Matroska EBML element IDs.
//!
//! Only the subset needed for demuxing is defined here.

// EBML Header
pub const EBML: u32 = 0x1A45DFA3;
pub const EBML_VERSION: u32 = 0x4286;
pub const EBML_DOC_TYPE: u32 = 0x4282;

// Segment
pub const SEGMENT: u32 = 0x18538067;

// Segment Information
pub const INFO: u32 = 0x1549A966;
pub const TIMESTAMP_SCALE: u32 = 0x2AD7B1;
pub const DURATION: u32 = 0x4489;

// Tracks
pub const TRACKS: u32 = 0x1654AE6B;
pub const TRACK_ENTRY: u32 = 0xAE;
pub const TRACK_NUMBER: u32 = 0xD7;
pub const TRACK_TYPE: u32 = 0x83;
pub const CODEC_ID: u32 = 0x86;
pub const CODEC_PRIVATE: u32 = 0x63A2;

// Video
pub const VIDEO: u32 = 0xE0;
pub const PIXEL_WIDTH: u32 = 0xB0;
pub const PIXEL_HEIGHT: u32 = 0xBA;

// Audio
pub const AUDIO: u32 = 0xE1;
pub const SAMPLING_FREQUENCY: u32 = 0xB5;
pub const CHANNELS: u32 = 0x9F;

// Cluster
pub const CLUSTER: u32 = 0x1F43B675;
pub const CLUSTER_TIMESTAMP: u32 = 0xE7;
pub const SIMPLE_BLOCK: u32 = 0xA3;

// Track type values
pub const TRACK_TYPE_VIDEO: u64 = 1;
pub const TRACK_TYPE_AUDIO: u64 = 2;
