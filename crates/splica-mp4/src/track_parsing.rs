//! Track parsing logic extracted from the demuxer.
//!
//! Parses a `trak` box body into an `Mp4Track`, resolving the track header,
//! media header, handler, and sample table sub-boxes.

use crate::boxes::{
    ctts, find_box, hdlr, mdhd, mvhd, require_box, stco, stsc, stsd, stss, stsz, stts, tkhd,
    FourCC,
};
use crate::error::Mp4Error;
use crate::sample_table;
use crate::track::Mp4Track;

/// Parses a single `trak` box into an [`Mp4Track`].
///
/// Reads all required sub-boxes (tkhd, mdia, mdhd, hdlr, stbl, etc.) and
/// builds the sample table. The `movie_header` is used as a fallback for
/// computing track duration when the media header duration is zero.
pub(crate) fn parse_track(
    trak_body: &[u8],
    base_offset: u64,
    movie_header: &mvhd::MovieHeaderBox,
) -> Result<Mp4Track, Mp4Error> {
    let tkhd_box = require_box(trak_body, FourCC::TKHD, base_offset, "tkhd")?;
    let track_header = tkhd::parse_tkhd(tkhd_box.body, tkhd_box.offset)?;

    let mdia_box = require_box(trak_body, FourCC::MDIA, base_offset, "mdia")?;
    let mdia = mdia_box.body;

    let mdhd_box = require_box(mdia, FourCC::MDHD, base_offset, "mdhd")?;
    let media_header = mdhd::parse_mdhd(mdhd_box.body, mdhd_box.offset)?;

    let hdlr_box = require_box(mdia, FourCC::HDLR, base_offset, "hdlr")?;
    let handler = hdlr::parse_hdlr(hdlr_box.body, hdlr_box.offset)?;

    let minf_box = require_box(mdia, FourCC::MINF, base_offset, "minf")?;
    let stbl_box = require_box(minf_box.body, FourCC::STBL, base_offset, "stbl")?;
    let stbl = stbl_box.body;

    // Parse sample table boxes
    let stsd_box = require_box(stbl, FourCC::STSD, base_offset, "stsd")?;
    let codec_config = stsd::parse_stsd(stsd_box.body, stsd_box.offset)?;

    let stts_box = require_box(stbl, FourCC::STTS, base_offset, "stts")?;
    let time_to_sample = stts::parse_stts(stts_box.body, stts_box.offset)?;

    let ctts_data = find_box(stbl, FourCC::CTTS, base_offset)?;
    let composition_offset = ctts_data
        .map(|b| ctts::parse_ctts(b.body, b.offset))
        .transpose()?;

    let stsc_box = require_box(stbl, FourCC::STSC, base_offset, "stsc")?;
    let sample_to_chunk = stsc::parse_stsc(stsc_box.body, stsc_box.offset)?;

    let stsz_box = require_box(stbl, FourCC::STSZ, base_offset, "stsz")?;
    let sample_sizes = stsz::parse_stsz(stsz_box.body, stsz_box.offset)?;

    // Try stco first, fall back to co64
    let chunk_offsets = if let Some(stco_box) = find_box(stbl, FourCC::STCO, base_offset)? {
        stco::parse_stco(stco_box.body, stco_box.offset)?
    } else if let Some(co64_box) = find_box(stbl, FourCC::CO64, base_offset)? {
        stco::parse_co64(co64_box.body, co64_box.offset)?
    } else {
        return Err(Mp4Error::MissingBox { name: "stco/co64" });
    };

    let stss_data = find_box(stbl, FourCC::STSS, base_offset)?;
    let sync_samples = stss_data
        .map(|b| stss::parse_stss(b.body, b.offset))
        .transpose()?;

    let sample_table = sample_table::build_sample_table(
        &time_to_sample,
        composition_offset.as_ref(),
        &sample_to_chunk,
        &sample_sizes,
        &chunk_offsets,
        sync_samples.as_ref(),
        media_header.timescale,
    )?;

    // Compute duration: prefer mdhd duration, fall back to tkhd duration scaled
    let duration = if media_header.duration > 0 {
        media_header.duration
    } else if track_header.duration > 0 && movie_header.timescale > 0 {
        // tkhd duration is in movie timescale — rescale to media timescale
        track_header.duration * media_header.timescale as u64 / movie_header.timescale as u64
    } else {
        0
    };

    Ok(Mp4Track {
        track_id: track_header.track_id,
        handler_type: handler.handler_type,
        timescale: media_header.timescale,
        duration,
        codec_config,
        sample_table,
        width: track_header.width,
        height: track_header.height,
    })
}
