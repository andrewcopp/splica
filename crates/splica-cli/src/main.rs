use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use miette::{Context, IntoDiagnostic, Result};

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Convert { input, output } => convert(&input, &output),
    }
}

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

    // Get tracks from demuxer and register them with the muxer
    // We need the internal mp4 tracks for codec config, so use the Muxer trait
    let track_count = demuxer.tracks().len();
    for i in 0..track_count {
        let info = demuxer.tracks()[i].clone();
        muxer
            .add_track(&info)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to add track {i}"))?;
    }

    // Copy all packets
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
