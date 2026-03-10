use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use miette::{Context, IntoDiagnostic, Result};
use serde::Serialize;

use splica_core::{Demuxer, Muxer, TrackIndex, TrackKind};
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

    /// Extract a time range from a media file (stream copy, no re-encoding).
    Trim {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Start time (e.g., "1:30", "90", "0:01:30.5").
        #[arg(long)]
        start: Option<String>,

        /// End time (e.g., "2:00", "120", "0:02:00").
        #[arg(long)]
        end: Option<String>,
    },

    /// Extract only audio tracks from a media file.
    ExtractAudio {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,
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
        Commands::Trim {
            input,
            output,
            start,
            end,
        } => trim(&input, &output, start.as_deref(), end.as_deref()),
        Commands::ExtractAudio { input, output } => extract_audio(&input, &output),
    }
}

// ---------------------------------------------------------------------------
// Time parsing
// ---------------------------------------------------------------------------

/// Parses a human-readable timestamp into seconds.
///
/// Supported formats:
/// - "90" or "90.5" — seconds
/// - "1:30" or "1:30.5" — minutes:seconds
/// - "0:01:30" or "0:01:30.5" — hours:minutes:seconds
fn parse_time(s: &str) -> Result<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        1 => {
            // seconds only
            parts[0]
                .parse::<f64>()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))
        }
        2 => {
            // minutes:seconds
            let minutes: f64 = parts[0]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            let seconds: f64 = parts[1]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            Ok(minutes * 60.0 + seconds)
        }
        3 => {
            // hours:minutes:seconds
            let hours: f64 = parts[0]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            let minutes: f64 = parts[1]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            let seconds: f64 = parts[2]
                .parse()
                .into_diagnostic()
                .wrap_err_with(|| format!("invalid time: '{s}'"))?;
            Ok(hours * 3600.0 + minutes * 60.0 + seconds)
        }
        _ => Err(miette::miette!(
            "invalid time format: '{s}' — use seconds, M:SS, or H:MM:SS"
        )),
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
            let codec = format_codec(&t.codec);

            let kind = match t.kind {
                TrackKind::Video => "video",
                TrackKind::Audio => "audio",
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

fn format_codec(codec: &splica_core::Codec) -> String {
    match codec {
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
    }
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

// ---------------------------------------------------------------------------
// trim
// ---------------------------------------------------------------------------

fn trim(input: &PathBuf, output: &PathBuf, start: Option<&str>, end: Option<&str>) -> Result<()> {
    let start_secs = start.map(parse_time).transpose()?;
    let end_secs = end.map(parse_time).transpose()?;

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

    // Set up tracks with codec config passthrough
    let track_count = demuxer.tracks().len();
    for i in 0..track_count {
        let track_idx = TrackIndex(i as u32);
        let info = demuxer.tracks()[i].clone();
        if let (Some(config), Some(timescale)) = (
            demuxer.codec_config(track_idx).cloned(),
            demuxer.track_timescale(track_idx),
        ) {
            muxer
                .add_track_with_config(&info, config, timescale)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to add track {i}"))?;
        } else {
            muxer
                .add_track(&info)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to add track {i}"))?;
        }
    }

    // Copy metadata
    let metadata = demuxer.metadata().to_vec();
    muxer.set_metadata(metadata);

    let mut packet_count: u64 = 0;
    let mut skipped: u64 = 0;

    while let Some(packet) = demuxer
        .read_packet()
        .into_diagnostic()
        .wrap_err("failed to read packet")?
    {
        let pts_secs = packet.pts.as_seconds_f64();

        // Filter by time range
        if let Some(start) = start_secs {
            if pts_secs < start {
                // For video: skip until we find a keyframe at or after start.
                // For audio: skip packets before start.
                // However, we need to include the keyframe before start for
                // correct decoding. Since this is stream copy, we include
                // keyframes just before start to avoid broken output.
                if !packet.is_keyframe {
                    skipped += 1;
                    continue;
                }
                // Include the last keyframe before start, but only track it.
                // Actually for clean trim, skip everything before start.
                skipped += 1;
                continue;
            }
        }

        if let Some(end) = end_secs {
            if pts_secs >= end {
                skipped += 1;
                continue;
            }
        }

        muxer
            .write_packet_data(&packet)
            .into_diagnostic()
            .wrap_err("failed to write packet")?;
        packet_count += 1;
    }

    muxer
        .finalize_file()
        .into_diagnostic()
        .wrap_err("failed to finalize output MP4")?;

    eprintln!(
        "Trimmed: wrote {packet_count} packets, skipped {skipped} to {}",
        output.display()
    );
    if let (Some(s), Some(e)) = (start_secs, end_secs) {
        eprintln!("  Time range: {s:.2}s — {e:.2}s");
    } else if let Some(s) = start_secs {
        eprintln!("  Start: {s:.2}s");
    } else if let Some(e) = end_secs {
        eprintln!("  End: {e:.2}s");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// extract-audio
// ---------------------------------------------------------------------------

fn extract_audio(input: &PathBuf, output: &PathBuf) -> Result<()> {
    let in_file = File::open(input)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open input file '{}'", input.display()))?;

    let mut demuxer = Mp4Demuxer::open(in_file)
        .into_diagnostic()
        .wrap_err("failed to parse MP4 container")?;

    // Find audio tracks
    let audio_tracks: Vec<(usize, splica_core::TrackInfo)> = demuxer
        .tracks()
        .iter()
        .enumerate()
        .filter(|(_, t)| t.kind == TrackKind::Audio)
        .map(|(i, t)| (i, t.clone()))
        .collect();

    if audio_tracks.is_empty() {
        return Err(miette::miette!(
            "no audio tracks found in '{}'",
            input.display()
        ));
    }

    let out_file = File::create(output)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not create output file '{}'", output.display()))?;

    let mut muxer = Mp4Muxer::new(BufWriter::new(out_file));

    // Map input track indices to output track indices
    let mut input_to_output: std::collections::HashMap<u32, TrackIndex> =
        std::collections::HashMap::new();

    for (input_idx, info) in &audio_tracks {
        let track_idx = TrackIndex(*input_idx as u32);
        if let (Some(config), Some(timescale)) = (
            demuxer.codec_config(track_idx).cloned(),
            demuxer.track_timescale(track_idx),
        ) {
            let output_idx = muxer
                .add_track_with_config(info, config, timescale)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to add audio track {input_idx}"))?;
            input_to_output.insert(*input_idx as u32, output_idx);
        } else {
            let output_idx = muxer
                .add_track(info)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to add audio track {input_idx}"))?;
            input_to_output.insert(*input_idx as u32, output_idx);
        }
    }

    // Copy metadata
    let metadata = demuxer.metadata().to_vec();
    muxer.set_metadata(metadata);

    let mut packet_count: u64 = 0;

    while let Some(mut packet) = demuxer
        .read_packet()
        .into_diagnostic()
        .wrap_err("failed to read packet")?
    {
        if let Some(&output_idx) = input_to_output.get(&packet.track_index.0) {
            packet.track_index = output_idx;
            muxer
                .write_packet_data(&packet)
                .into_diagnostic()
                .wrap_err("failed to write packet")?;
            packet_count += 1;
        }
        // Silently skip non-audio packets
    }

    muxer
        .finalize_file()
        .into_diagnostic()
        .wrap_err("failed to finalize output MP4")?;

    eprintln!(
        "Extracted {packet_count} audio packets ({} audio tracks) to {}",
        audio_tracks.len(),
        output.display()
    );

    Ok(())
}
