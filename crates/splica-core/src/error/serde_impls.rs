//! Serde `Serialize` implementations for error types.
//!
//! Extracted from the main error module to keep file sizes manageable.
//! Each error type serializes to a structured JSON map with common fields
//! (variant, kind, is_retryable, message) plus type-specific extras.

use serde::ser::{SerializeMap, Serializer};

use super::{
    DecodeError, DemuxError, EncodeError, ErrorKind, FilterError, MuxError, PipelineError,
};

// ---------------------------------------------------------------------------
// Serde helper: serializes any error type that has kind() and Display
// ---------------------------------------------------------------------------

/// Start a map with the common fields: variant, kind, is_retryable, message.
fn start_error_map<S: Serializer>(
    serializer: S,
    variant: &str,
    kind: ErrorKind,
    message: &str,
    extra_fields: usize,
) -> Result<S::SerializeMap, S::Error> {
    let mut map = serializer.serialize_map(Some(4 + extra_fields))?;
    map.serialize_entry("variant", variant)?;
    map.serialize_entry("kind", &kind)?;
    map.serialize_entry("is_retryable", &kind.is_retryable())?;
    map.serialize_entry("message", message)?;
    Ok(map)
}

// ---------------------------------------------------------------------------
// DemuxError
// ---------------------------------------------------------------------------

impl serde::Serialize for DemuxError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let msg = self.to_string();
        match self {
            Self::InvalidContainer { offset, .. } => {
                let mut map =
                    start_error_map(serializer, "InvalidContainer", self.kind(), &msg, 1)?;
                map.serialize_entry("offset", offset)?;
                map.end()
            }
            Self::UnsupportedCodec { codec } => {
                let mut map =
                    start_error_map(serializer, "UnsupportedCodec", self.kind(), &msg, 1)?;
                map.serialize_entry("codec", codec)?;
                map.end()
            }
            Self::UnexpectedEof { offset } => {
                let mut map = start_error_map(serializer, "UnexpectedEof", self.kind(), &msg, 1)?;
                map.serialize_entry("offset", offset)?;
                map.end()
            }
            Self::Io(_) => {
                let map = start_error_map(serializer, "Io", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Other(_) => {
                let map = start_error_map(serializer, "Other", self.kind(), &msg, 0)?;
                map.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DecodeError
// ---------------------------------------------------------------------------

impl serde::Serialize for DecodeError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let msg = self.to_string();
        match self {
            Self::InvalidBitstream { .. } => {
                let map = start_error_map(serializer, "InvalidBitstream", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::UnsupportedProfile { codec, profile } => {
                let mut map =
                    start_error_map(serializer, "UnsupportedProfile", self.kind(), &msg, 2)?;
                map.serialize_entry("codec", codec)?;
                map.serialize_entry("profile", profile)?;
                map.end()
            }
            Self::ResourceExhausted { .. } => {
                let map = start_error_map(serializer, "ResourceExhausted", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Io(_) => {
                let map = start_error_map(serializer, "Io", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Other(_) => {
                let map = start_error_map(serializer, "Other", self.kind(), &msg, 0)?;
                map.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// EncodeError
// ---------------------------------------------------------------------------

impl serde::Serialize for EncodeError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let msg = self.to_string();
        match self {
            Self::InvalidFrame { .. } => {
                let map = start_error_map(serializer, "InvalidFrame", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::UnsupportedConfig { .. } => {
                let map = start_error_map(serializer, "UnsupportedConfig", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::ResourceExhausted { .. } => {
                let map = start_error_map(serializer, "ResourceExhausted", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Io(_) => {
                let map = start_error_map(serializer, "Io", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Other(_) => {
                let map = start_error_map(serializer, "Other", self.kind(), &msg, 0)?;
                map.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MuxError
// ---------------------------------------------------------------------------

impl serde::Serialize for MuxError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let msg = self.to_string();
        match self {
            Self::InvalidTrackConfig { .. } => {
                let map = start_error_map(serializer, "InvalidTrackConfig", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::IncompatibleCodec { codec, container } => {
                let mut map =
                    start_error_map(serializer, "IncompatibleCodec", self.kind(), &msg, 2)?;
                map.serialize_entry("codec", codec)?;
                map.serialize_entry("container", container)?;
                map.end()
            }
            Self::ResourceExhausted { .. } => {
                let map = start_error_map(serializer, "ResourceExhausted", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Io(_) => {
                let map = start_error_map(serializer, "Io", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Other(_) => {
                let map = start_error_map(serializer, "Other", self.kind(), &msg, 0)?;
                map.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FilterError
// ---------------------------------------------------------------------------

impl serde::Serialize for FilterError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let msg = self.to_string();
        match self {
            Self::InvalidInput { .. } => {
                let map = start_error_map(serializer, "InvalidInput", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::ResourceExhausted { .. } => {
                let map = start_error_map(serializer, "ResourceExhausted", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Other(_) => {
                let map = start_error_map(serializer, "Other", self.kind(), &msg, 0)?;
                map.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PipelineError
// ---------------------------------------------------------------------------

impl serde::Serialize for PipelineError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let msg = self.to_string();
        match self {
            Self::Demux(inner) => {
                let mut map = start_error_map(serializer, "Demux", self.kind(), &msg, 1)?;
                map.serialize_entry("inner", inner)?;
                map.end()
            }
            Self::Decode(inner) => {
                let mut map = start_error_map(serializer, "Decode", self.kind(), &msg, 1)?;
                map.serialize_entry("inner", inner)?;
                map.end()
            }
            Self::Filter(inner) => {
                let mut map = start_error_map(serializer, "Filter", self.kind(), &msg, 1)?;
                map.serialize_entry("inner", inner)?;
                map.end()
            }
            Self::Encode(inner) => {
                let mut map = start_error_map(serializer, "Encode", self.kind(), &msg, 1)?;
                map.serialize_entry("inner", inner)?;
                map.end()
            }
            Self::Mux(inner) => {
                let mut map = start_error_map(serializer, "Mux", self.kind(), &msg, 1)?;
                map.serialize_entry("inner", inner)?;
                map.end()
            }
            Self::Config { .. } => {
                let map = start_error_map(serializer, "Config", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Validation(_) => {
                let map = start_error_map(serializer, "Validation", self.kind(), &msg, 0)?;
                map.end()
            }
            Self::Other(_) => {
                let map = start_error_map(serializer, "Other", self.kind(), &msg, 0)?;
                map.end()
            }
        }
    }
}
