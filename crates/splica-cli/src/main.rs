use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use miette::{Context, IntoDiagnostic, Result};
use serde::Serialize;

use splica_core::{Demuxer, Muxer};
use splica_mp4::{Mp4Demuxer, Mp4Muxer};

#[derive(Parser)]
#[command(name = "splica", version, about = "Media processing tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Copy streams from input to output without re-encoding.
    Convert {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Inspect a media file and print track information.
    Probe {
        /// Input file path.
        file: PathBuf,

        /// Output format.
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Convert { input, output } => convert(&input, &output),
        Commands::Probe { file, format } => probe(&file, &format),
    }
}

// ---------------------------------------------------------------------------
// probe
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ProbeOutput {
    file: String,
    tracks: Vec<ProbeTrack>,
}

#[derive(Serialize)]
struct ProbeTrack {
    index: u32,
    kind: String,
    codec: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame_rate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pixel_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_layout: Option<String>,
}

fn probe(file: &PathBuf, format: &OutputFormat) -> Result<()> {
    let in_file = File::open(file)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open file '{}'", file.display()))?;

    let demuxer = Mp4Demuxer::open(in_file)
        .into_diagnostic()
        .wrap_err("failed to parse MP4 container")?;

    let tracks: Vec<ProbeTrack> = demuxer
        .tracks()
        .iter()
        .map(|t| {
            let codec = match &t.codec {
                splica_core::Codec::Video(vc) => match vc {
                    splica_core::VideoCodec::H264 => "H.264".to_string(),
                    splica_core::VideoCodec::H265 => "H.265".to_string(),
                    splica_core::VideoCodec::Av1 => "AV1".to_string(),
                    splica_core::VideoCodec::Other(s) => s.clone(),
                },
                splica_core::Codec::Audio(ac) => match ac {
                    splica_core::AudioCodec::Aac => "AAC".to_string(),
                    splica_core::AudioCodec::Opus => "Opus".to_string(),
                    splica_core::AudioCodec::Other(s) => s.clone(),
                },
            };

            let kind = match t.kind {
                splica_core::TrackKind::Video => "video",
                splica_core::TrackKind::Audio => "audio",
            };

            ProbeTrack {
                index: t.index.0,
                kind: kind.to_string(),
                codec,
                duration_seconds: t.duration.map(|d| d.as_seconds_f64()),
                width: t.video.as_ref().map(|v| v.width),
                height: t.video.as_ref().map(|v| v.height),
                frame_rate: t
                    .video
                    .as_ref()
                    .and_then(|v| v.frame_rate.map(|fr| fr.to_string())),
                pixel_format: t
                    .video
                    .as_ref()
                    .and_then(|v| v.pixel_format.map(|pf| format!("{pf:?}"))),
                sample_rate: t.audio.as_ref().map(|a| a.sample_rate),
                channel_layout: t
                    .audio
                    .as_ref()
                    .and_then(|a| a.channel_layout.map(|cl| format!("{cl:?}"))),
            }
        })
        .collect();

    match format {
        OutputFormat::Json => {
            let output = ProbeOutput {
                file: file.display().to_string(),
                tracks,
            };
            let json = serde_json::to_string_pretty(&output).into_diagnostic()?;
            println!("{json}");
        }
        OutputFormat::Text => {
            println!("File: {}", file.display());
            println!("Tracks: {}", tracks.len());
            println!();
            for track in &tracks {
                println!("  Track {} ({}): {}", track.index, track.kind, track.codec);
                if let (Some(w), Some(h)) = (track.width, track.height) {
                    print!("    Resolution: {w}x{h}");
                    if let Some(ref fr) = track.frame_rate {
                        print!(" @ {fr} fps");
                    }
                    println!();
                }
                if let Some(ref pf) = track.pixel_format {
                    println!("    Pixel format: {pf}");
                }
                if let Some(sr) = track.sample_rate {
                    print!("    Sample rate: {sr} Hz");
                    if let Some(ref cl) = track.channel_layout {
                        print!(", {cl}");
                    }
                    println!();
                }
                if let Some(dur) = track.duration_seconds {
                    let mins = (dur / 60.0).floor() as u64;
                    let secs = dur % 60.0;
                    println!("    Duration: {mins}:{secs:05.2}");
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// convert
// ---------------------------------------------------------------------------

fn convert(input: &PathBuf, output: &PathBuf) -> Result<()> {
    let in_file = File::open(input)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open input file '{}'", input.display()))?;

    let mut demuxer = Mp4Demuxer::open(in_file)
        .into_diagnostic()
        .wrap_err("failed to parse MP4 container")?;

    let out_file = File::create(output)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not create output file '{}'", output.display()))?;

    let mut muxer = Mp4Muxer::new(BufWriter::new(out_file));

    let track_count = demuxer.tracks().len();
    for i in 0..track_count {
        let info = demuxer.tracks()[i].clone();
        muxer
            .add_track(&info)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to add track {i}"))?;
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

    muxer
        .finalize()
        .into_diagnostic()
        .wrap_err("failed to finalize output MP4")?;

    eprintln!(
        "Copied {packet_count} packets across {track_count} tracks to {}",
        output.display()
    );

    Ok(())
}
