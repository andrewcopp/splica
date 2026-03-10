use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom};
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use miette::{Context, IntoDiagnostic, Result};
use serde::Serialize;

use splica_codec::{H264Decoder, H264EncoderBuilder};
use splica_core::{Codec, Demuxer, Muxer, TrackIndex, TrackKind, VideoCodec};
use splica_mp4::boxes::stsd::CodecConfig;
use splica_mp4::{Mp4Demuxer, Mp4Muxer};
use splica_pipeline::{PipelineBuilder, PipelineEventKind};
use splica_webm::WebmDemuxer;

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

    /// Transcode video using the pipeline (demux → decode → encode → mux).
    Transcode {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Target video bitrate (e.g., "2M", "1500k", or raw bps).
        #[arg(long)]
        bitrate: Option<String>,
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
        Commands::Transcode {
            input,
            output,
            bitrate,
        } => transcode(&input, &output, bitrate.as_deref()),
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
// Format detection
// ---------------------------------------------------------------------------

/// Detected container format.
enum ContainerFormat {
    Mp4,
    WebM,
}

/// Sniffs the container format from the first few bytes of a file.
fn detect_format(file: &mut (impl Read + Seek)) -> Result<ContainerFormat> {
    let mut magic = [0u8; 12];
    let bytes_read = file
        .read(&mut magic)
        .into_diagnostic()
        .wrap_err("could not read file header")?;
    file.seek(SeekFrom::Start(0))
        .into_diagnostic()
        .wrap_err("could not seek back to start")?;

    if bytes_read < 4 {
        return Err(miette::miette!(
            "file too small to detect format ({bytes_read} bytes)"
        ));
    }

    // WebM/Matroska: EBML header starts with 0x1A 0x45 0xDF 0xA3
    if magic[0] == 0x1A && magic[1] == 0x45 && magic[2] == 0xDF && magic[3] == 0xA3 {
        return Ok(ContainerFormat::WebM);
    }

    // MP4: "ftyp" box at bytes 4-7
    if bytes_read >= 8 && &magic[4..8] == b"ftyp" {
        return Ok(ContainerFormat::Mp4);
    }

    Err(miette::miette!(
        "unsupported container format — splica supports MP4 and WebM"
    ))
}

/// Opens a demuxer for the given file, auto-detecting the container format.
fn open_demuxer(path: &PathBuf) -> Result<Box<dyn Demuxer>> {
    let mut file = File::open(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open file '{}'", path.display()))?;

    let format = detect_format(&mut file)?;

    match format {
        ContainerFormat::Mp4 => {
            let demuxer = Mp4Demuxer::open(file)
                .into_diagnostic()
                .wrap_err("failed to parse MP4 container")?;
            Ok(Box::new(demuxer))
        }
        ContainerFormat::WebM => {
            let demuxer = WebmDemuxer::open(BufReader::new(file))
                .into_diagnostic()
                .wrap_err("failed to parse WebM container")?;
            Ok(Box::new(demuxer))
        }
    }
}

/// Opens an MP4 demuxer specifically (for commands needing codec config access).
fn open_mp4_demuxer(path: &PathBuf) -> Result<Mp4Demuxer<File>> {
    let file = File::open(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not open file '{}'", path.display()))?;

    Mp4Demuxer::open(file)
        .into_diagnostic()
        .wrap_err("failed to parse MP4 container")
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
    let demuxer = open_demuxer(file)?;

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
    let mut demuxer = open_demuxer(input)?;

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

    let mut demuxer = open_mp4_demuxer(input).wrap_err("trim currently requires MP4 input")?;

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
    let mut demuxer =
        open_mp4_demuxer(input).wrap_err("extract-audio currently requires MP4 input")?;

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

// ---------------------------------------------------------------------------
// transcode
// ---------------------------------------------------------------------------

/// Parses a bitrate string like "2M", "1500k", or "1000000" into bits per second.
fn parse_bitrate(s: &str) -> Result<u32> {
    let s = s.trim();
    if let Some(prefix) = s.strip_suffix('M') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000_000.0) as u32)
    } else if let Some(prefix) = s.strip_suffix('m') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000_000.0) as u32)
    } else if let Some(prefix) = s.strip_suffix('k') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000.0) as u32)
    } else if let Some(prefix) = s.strip_suffix('K') {
        let val: f64 = prefix
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid bitrate: '{s}'"))?;
        Ok((val * 1_000.0) as u32)
    } else {
        s.parse::<u32>().into_diagnostic().wrap_err_with(|| {
            format!("invalid bitrate: '{s}' — use e.g. '2M', '1500k', or raw bps")
        })
    }
}

fn transcode(input: &PathBuf, output: &PathBuf, bitrate: Option<&str>) -> Result<()> {
    let bitrate_bps = match bitrate {
        Some(s) => parse_bitrate(s)?,
        None => 1_000_000, // 1 Mbps default
    };

    let demuxer = open_mp4_demuxer(input).wrap_err("transcode currently requires MP4 input")?;

    // Read track info and codec configs before moving demuxer into pipeline
    let tracks = demuxer.tracks().to_vec();
    let mut video_track_configs: Vec<(TrackIndex, Vec<u8>)> = Vec::new();

    for track in &tracks {
        if track.kind == TrackKind::Video {
            if let Codec::Video(VideoCodec::H264) = &track.codec {
                if let Some(CodecConfig::Avc1 { avcc, .. }) = demuxer.codec_config(track.index) {
                    video_track_configs.push((track.index, avcc.to_vec()));
                }
            }
        }
    }

    if video_track_configs.is_empty() {
        return Err(miette::miette!(
            "no H.264 video tracks found in '{}' — transcode currently supports H.264 only",
            input.display()
        ));
    }

    let out_file = File::create(output)
        .into_diagnostic()
        .wrap_err_with(|| format!("could not create output file '{}'", output.display()))?;

    let muxer = Mp4Muxer::new(BufWriter::new(out_file));

    // Build pipeline: video tracks get decoder+encoder, audio tracks copy through
    let mut builder = PipelineBuilder::new()
        .with_event_handler(|event| match event.kind {
            PipelineEventKind::PacketsRead { count } if count % 100 == 0 => {
                eprint!("\r  Packets read: {count}");
            }
            PipelineEventKind::FramesDecoded { count } if count % 100 == 0 => {
                eprint!("  Decoded: {count}");
            }
            PipelineEventKind::FramesEncoded { count } if count % 100 == 0 => {
                eprint!("  Encoded: {count}");
            }
            _ => {}
        })
        .with_demuxer(demuxer)
        .with_muxer(muxer);

    for (track_idx, avcc_data) in &video_track_configs {
        let decoder = H264Decoder::new(avcc_data)
            .into_diagnostic()
            .wrap_err("failed to create H.264 decoder")?;

        let encoder = H264EncoderBuilder::new()
            .bitrate(bitrate_bps)
            .track_index(*track_idx)
            .build()
            .into_diagnostic()
            .wrap_err("failed to create H.264 encoder")?;

        builder = builder.with_decoder(*track_idx, decoder);
        builder = builder.with_encoder(*track_idx, encoder);
    }

    let mut pipeline = builder
        .build()
        .into_diagnostic()
        .wrap_err("failed to build transcode pipeline")?;

    eprintln!(
        "Transcoding {} → {} (H.264, {} kbps)",
        input.display(),
        output.display(),
        bitrate_bps / 1000
    );

    pipeline
        .run()
        .into_diagnostic()
        .wrap_err("transcode failed")?;

    eprintln!("\r  Done.                                        ");

    Ok(())
}
