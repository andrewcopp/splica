use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use miette::{Context, IntoDiagnostic, Result};

use splica_core::{ContainerFormat, Demuxer, Muxer};
use splica_mkv::{MkvDemuxer, MkvMuxer};
use splica_mp4::{Mp4Demuxer, Mp4Muxer};
use splica_webm::{WebmDemuxer, WebmMuxer};

use super::format_detect::{detect_format, DetectedFormat};

// ---------------------------------------------------------------------------
// Output format validation
// ---------------------------------------------------------------------------

/// Validates that the output file extension is a format splica can write.
/// Fails fast before any input is read, with a helpful error message.
pub(crate) fn validate_output_format(output: &Path) -> Result<()> {
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ContainerFormat::from_extension(ext) {
        Some(fmt) if fmt.is_writable() => Ok(()),
        Some(_) => Ok(()), // unreachable since all recognized formats are writable
        None if ext.is_empty() => Err(miette::miette!(
            "output file has no extension — cannot determine output format\n  \
             → Use a recognized extension: .mp4, .webm, .mkv"
        )),
        None => Err(miette::miette!(
            "unsupported output format '.{ext}'\n  \
             → splica can currently write: mp4, webm, mkv\n  \
             → Supported extensions: .mp4, .m4v, .m4a, .webm, .mkv, .mka"
        )),
    }
}

// ---------------------------------------------------------------------------
// Container helpers
// ---------------------------------------------------------------------------

pub(crate) fn output_container(path: &Path) -> Option<ContainerFormat> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    ContainerFormat::from_extension(ext)
}

/// Opens a demuxer for the given file, auto-detecting the container format.
pub(crate) fn open_demuxer(path: &Path) -> Result<Box<dyn Demuxer>> {
    let mut file = File::open(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open file '{}'", path.display()))?;

    let format = detect_format(&mut file)?;

    match format {
        DetectedFormat::Mp4 => {
            let demuxer = Mp4Demuxer::open(file)
                .into_diagnostic()
                .wrap_err("failed to parse MP4 container")?;
            Ok(Box::new(demuxer))
        }
        DetectedFormat::WebM => {
            let demuxer = WebmDemuxer::open(BufReader::new(file))
                .into_diagnostic()
                .wrap_err("failed to parse WebM container")?;
            Ok(Box::new(demuxer))
        }
        DetectedFormat::Mkv => {
            let demuxer = MkvDemuxer::open(BufReader::new(file))
                .into_diagnostic()
                .wrap_err("failed to parse MKV container")?;
            Ok(Box::new(demuxer))
        }
    }
}

/// Creates an output muxer based on file extension.
pub(crate) fn create_muxer(output: &Path) -> Result<Box<dyn Muxer>> {
    let out_file = File::create(output)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not create output file '{}'", output.display()))?;

    match output_container(output) {
        Some(ContainerFormat::WebM) => Ok(Box::new(WebmMuxer::new(BufWriter::new(out_file)))),
        Some(ContainerFormat::Mkv) => Ok(Box::new(MkvMuxer::new(BufWriter::new(out_file)))),
        _ => Ok(Box::new(Mp4Muxer::new(BufWriter::new(out_file)))),
    }
}
