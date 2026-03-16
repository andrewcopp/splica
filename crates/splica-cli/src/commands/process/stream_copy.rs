use std::fs::File;
use std::io::BufWriter;

use miette::{Context, IntoDiagnostic, Result};

use splica_core::{Muxer, TrackKind};
use splica_mkv::MkvMuxer;
use splica_mp4::Mp4Muxer;
use splica_webm::WebmMuxer;

use super::args::ProcessArgs;
use super::reencode::probe_output_qc;
use super::TranscodeOutput;
use crate::commands::{
    open_demuxer, output_container, AudioMode, ContainerFormat, TranscodeAudioInfo,
};

/// Stream copy path: copies packets without re-encoding.
/// Returns TranscodeOutput with zero encode/decode counts.
pub(super) fn stream_copy(args: &ProcessArgs<'_>, json_mode: bool) -> Result<TranscodeOutput> {
    let mut demuxer = open_demuxer(args.input)?;

    let out_file = File::create(args.output)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not create output file '{}'", args.output.display()))?;

    let mut muxer: Box<dyn Muxer> = match output_container(args.output) {
        Some(ContainerFormat::WebM) => Box::new(WebmMuxer::new(BufWriter::new(out_file))),
        Some(ContainerFormat::Mkv) => Box::new(MkvMuxer::new(BufWriter::new(out_file))),
        _ => Box::new(Mp4Muxer::new(BufWriter::new(out_file))),
    };

    let tracks = demuxer.tracks().to_vec();
    let track_count = tracks.len();

    // Collect audio track info for JSON output
    let audio_tracks: Vec<TranscodeAudioInfo> = tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .map(|t| {
            let codec = t.codec.to_string();
            let sample_rate = t.audio.as_ref().map(|a| a.sample_rate).unwrap_or(0);
            let channels = t
                .audio
                .as_ref()
                .and_then(|a| a.channel_layout.map(|cl| cl.channel_count()));
            TranscodeAudioInfo {
                codec,
                sample_rate,
                channels,
                mode: AudioMode::PassThrough,
            }
        })
        .collect();

    for i in 0..track_count {
        let info = demuxer.tracks()[i].clone();
        muxer
            .add_track(&info)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to add track {i}"))?;
    }

    if !json_mode {
        eprintln!(
            "Processing {} → {} (stream copy)",
            args.input.display(),
            args.output.display()
        );
    }

    let mut packet_count: u64 = 0;
    while let Some(packet) = demuxer
        .read_packet()
        .into_diagnostic()
        .wrap_err("failed to read packet")?
    {
        muxer
            .write_packet(&packet)
            .into_diagnostic()
            .wrap_err("failed to write packet")?;
        packet_count += 1;
    }

    let mux_result = muxer.finalize();
    let mux_ok = mux_result.is_ok();

    if !json_mode {
        if mux_ok {
            eprintln!("  Done. Copied {packet_count} packets across {track_count} tracks.");
        }
        mux_result
            .into_diagnostic()
            .wrap_err("failed to finalize output")?;
    }

    let qc = probe_output_qc(args.output);

    Ok(TranscodeOutput {
        packets_read: packet_count,
        frames_decoded: 0,
        frames_encoded: 0,
        packets_written: packet_count,
        audio_tracks,
        mux_ok,
        output_codec: qc.codec,
        output_duration_secs: qc.duration_secs,
        output_bitrate_kbps: qc.bitrate_kbps,
        is_stream_copy: true,
        elapsed_secs: 0.0,
    })
}
