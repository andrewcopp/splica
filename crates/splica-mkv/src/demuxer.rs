//! MKV demuxer: reads a Matroska container and yields compressed packets.
//!
//! Matroska and WebM share the same EBML structure — WebM is a strict subset
//! of Matroska that only permits VP8/VP9/AV1 video and Vorbis/Opus audio.
//! MKV additionally supports H.264, H.265, AAC, and other codecs.
//!
//! This demuxer wraps `WebmDemuxer` (which already parses both "webm" and
//! "matroska" docTypes) and delegates all parsing. The wrapper exists so
//! callers can distinguish MKV from WebM at the type level and so the CLI
//! can route EBML files to the correct demuxer based on DocType.

use std::io::{Read, Seek};

use splica_core::{
    DemuxError, Demuxer, Packet, SeekMode, Seekable, Timestamp, TrackIndex, TrackInfo,
};
use splica_webm::WebmDemuxer;

use crate::error::MkvError;

/// An MKV demuxer that reads from any `Read + Seek` source.
///
/// Supports all Matroska codecs: H.264, H.265, VP8, VP9, AV1 (video)
/// and AAC, Opus, Vorbis (audio).
pub struct MkvDemuxer<R> {
    inner: WebmDemuxer<R>,
}

impl<R: Read + Seek> MkvDemuxer<R> {
    /// Opens an MKV file and parses its metadata.
    ///
    /// Returns `MkvError::NotMkv` if the EBML DocType is not "matroska".
    /// For WebM files (DocType "webm"), use `WebmDemuxer` instead.
    pub fn open(reader: R) -> Result<Self, MkvError> {
        let inner = WebmDemuxer::open(reader).map_err(|e| match e {
            splica_webm::WebmError::NotWebm => MkvError::NotMkv,
            splica_webm::WebmError::InvalidElement { offset, message } => {
                MkvError::InvalidElement { offset, message }
            }
            splica_webm::WebmError::UnexpectedEof { offset } => MkvError::UnexpectedEof { offset },
            splica_webm::WebmError::UnsupportedCodec { codec_id } => {
                MkvError::UnsupportedCodec { codec_id }
            }
            splica_webm::WebmError::MissingElement { name } => MkvError::MissingElement { name },
            splica_webm::WebmError::Io(e) => MkvError::Io(e),
        })?;

        Ok(Self { inner })
    }

    /// Returns the codec private data for a given track index, if present.
    pub fn codec_private(&self, track: TrackIndex) -> Option<&[u8]> {
        self.inner.codec_private(track)
    }

    /// Returns the presentation timestamp of the current read position.
    pub fn seek_position(&self) -> Option<Timestamp> {
        self.inner.seek_position()
    }
}

impl<R: Read + Seek> Demuxer for MkvDemuxer<R> {
    fn tracks(&self) -> &[TrackInfo] {
        self.inner.tracks()
    }

    fn read_packet(&mut self) -> Result<Option<Packet>, DemuxError> {
        self.inner.read_packet()
    }
}

impl<R: Read + Seek> Seekable for MkvDemuxer<R> {
    fn seek(&mut self, target: Timestamp, mode: SeekMode) -> Result<(), DemuxError> {
        self.inner.seek(target, mode)
    }
}
