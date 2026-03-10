//! Opaque metadata box passthrough for MP4 containers.
//!
//! Stores raw box bytes (including the box header) so they can be
//! round-tripped through demux → mux without interpretation.

use crate::boxes::FourCC;

/// An opaque metadata box extracted from the MP4 moov container.
///
/// Preserves the complete box data (header + body) for lossless
/// round-tripping. The `box_type` field identifies the box kind
/// (e.g., `udta`, `meta`) without requiring the caller to parse
/// the raw bytes.
#[derive(Debug, Clone)]
pub struct MetadataBox {
    /// The four-character code identifying this box.
    pub box_type: FourCC,
    /// The complete box data including header (size + fourcc + body).
    pub data: Vec<u8>,
}
