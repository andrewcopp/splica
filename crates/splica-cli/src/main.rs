mod commands;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use miette::Result;

use commands::process::ProcessArgs;
use commands::{
    AspectModeArg, AudioCodecArg, EncodePreset, H264LevelArg, H264ProfileArg, OutputFormat,
    VideoCodecArg,
};

#[derive(Parser)]
#[command(
    name = "splica",
    version,
    about = "Media processing tool",
    after_long_help = "\
EXIT CODES:
  0  Success
  1  Bad input — malformed file, unsupported format, or invalid arguments (do not retry)
  2  Internal error — encoder/muxer failure (may retry)
  3  Resource exhausted — memory, file handles, or budget limits (retry after backoff)

In --format json mode, errors include an \"error_kind\" field with one of:
  bad_input, unsupported_format, internal_error, resource_exhausted"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Process a media file: auto-selects stream copy or re-encode as needed.
    ///
    /// Uses stream copy when the input codecs are compatible with the output
    /// container. Falls back to re-encoding when codecs are incompatible or
    /// when encoding options (--resize, --bitrate, etc.) are specified.
    Process {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Target video bitrate (e.g., "2M", "1500k", or raw bps).
        /// Implies re-encoding. Mutually exclusive with --crf.
        #[arg(long, conflicts_with = "crf")]
        bitrate: Option<String>,

        /// Constant Rate Factor for perceptual quality (0–51, lower = better).
        /// Default: 23. Implies re-encoding. Mutually exclusive with --bitrate.
        #[arg(long, conflicts_with = "bitrate")]
        crf: Option<u8>,

        /// Encoding speed/quality preset. Implies re-encoding.
        #[arg(long)]
        preset: Option<EncodePreset>,

        /// Maximum frame rate hint for the encoder (e.g., 30, 60).
        /// Implies re-encoding.
        #[arg(long)]
        max_fps: Option<f32>,

        /// Resize video to WxH (e.g., "1280x720", "640x480").
        /// Implies re-encoding.
        #[arg(long)]
        resize: Option<String>,

        /// Aspect ratio handling when resizing (default: fit).
        #[arg(long, default_value = "fit")]
        aspect_mode: AspectModeArg,

        /// Crop video to WxH+X+Y (e.g., "1080x1080+420+0").
        /// Implies re-encoding.
        #[arg(long)]
        crop: Option<String>,

        /// Scale audio amplitude. Accepts a linear multiplier (e.g., "0.5" = half,
        /// "2.0" = double) or a dB value (e.g., "-6dB", "+3dB"). This is a
        /// straight gain control, not loudness normalization. Implies re-encoding audio.
        #[arg(long)]
        volume: Option<String>,

        /// Output video codec (e.g., "h264", "h265"). Implies re-encoding.
        /// Default: auto-select based on output container.
        #[arg(long)]
        codec: Option<VideoCodecArg>,

        /// Output audio codec (e.g., "aac", "opus").
        /// Default: auto-select based on output container (MP4→AAC, WebM/MKV→Opus).
        #[arg(long)]
        audio_codec: Option<AudioCodecArg>,

        /// Target audio bitrate (e.g., "128k", "256k", "192000").
        /// Default: 128kbps. Only applies when audio is re-encoded.
        #[arg(long)]
        audio_bitrate: Option<String>,

        /// H.264 encoding profile (baseline, main, high).
        /// Only valid when output codec is H.264. Default: auto (OpenH264 default).
        #[arg(long)]
        h264_profile: Option<H264ProfileArg>,

        /// H.264 encoding level (3.0, 3.1, 4.0, 4.1, 5.0, 5.1).
        /// Only valid when output codec is H.264. Default: auto.
        #[arg(long)]
        h264_level: Option<H264LevelArg>,

        /// Allow re-encoding when the input has non-standard color space metadata
        /// (e.g., HDR/BT.2020). Without this flag, splica will error rather than
        /// silently losing color information.
        #[arg(long)]
        allow_color_conversion: bool,

        /// Output format for results (text or json).
        #[arg(long, default_value = "text")]
        format: OutputFormat,
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

        /// Output format.
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// Concatenate multiple media files via stream copy.
    ///
    /// All input files must share the same codecs and track layout.
    /// Timestamps are remapped so the output plays sequentially.
    Join {
        /// Input file paths (at least 2 required).
        #[arg(short, long, num_args = 2.., required = true)]
        input: Vec<PathBuf>,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Output format.
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// Extract only audio tracks from a media file.
    ExtractAudio {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Output format.
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },

    /// Translate an ffmpeg command into the equivalent splica command.
    ///
    /// Accepts the ffmpeg command as trailing arguments. The "ffmpeg" prefix
    /// is optional:
    ///   splica migrate ffmpeg -i in.mp4 out.webm
    ///   splica migrate -i in.mp4 out.webm
    Migrate {
        /// The ffmpeg command to translate.
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },

    /// Deprecated: use `process` instead. Alias for stream-copy mode.
    #[command(hide = true)]
    Convert {
        /// Input file path.
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Deprecated: use `process` instead. Alias for re-encode mode.
    #[command(hide = true)]
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

        /// Encoding speed/quality preset.
        #[arg(long, default_value = "medium")]
        preset: EncodePreset,

        /// Maximum frame rate hint for the encoder (e.g., 30, 60).
        #[arg(long)]
        max_fps: Option<f32>,

        /// Resize video to WxH (e.g., "1280x720", "640x480").
        #[arg(long)]
        resize: Option<String>,

        /// Aspect ratio handling when resizing (default: fit).
        #[arg(long, default_value = "fit")]
        aspect_mode: AspectModeArg,

        /// Output format for results (text or json).
        #[arg(long, default_value = "text")]
        format: OutputFormat,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Process {
            input,
            output,
            bitrate,
            crf,
            preset,
            max_fps,
            resize,
            aspect_mode,
            crop,
            volume,
            codec,
            audio_codec,
            audio_bitrate,
            h264_profile,
            h264_level,
            allow_color_conversion,
            format,
        } => commands::process::process(
            &ProcessArgs {
                input: &input,
                output: &output,
                bitrate: bitrate.as_deref(),
                crf,
                preset: preset.as_ref(),
                max_fps,
                resize: resize.as_deref(),
                aspect_mode_arg: &aspect_mode,
                crop: crop.as_deref(),
                volume: volume.as_deref(),
                codec: codec.as_ref(),
                audio_codec: audio_codec.as_ref(),
                audio_bitrate: audio_bitrate.as_deref(),
                h264_profile: h264_profile.as_ref(),
                h264_level: h264_level.as_ref(),
                allow_color_conversion,
            },
            &format,
        ),
        Commands::Probe { file, format } => commands::probe::probe(&file, &format),
        Commands::Trim {
            input,
            output,
            start,
            end,
            format,
        } => commands::trim::trim(&input, &output, start.as_deref(), end.as_deref(), &format),
        Commands::Join {
            input,
            output,
            format,
        } => {
            let input_refs: Vec<&std::path::Path> = input.iter().map(|p| p.as_path()).collect();
            commands::join::join(&input_refs, &output, &format)
        }
        Commands::ExtractAudio {
            input,
            output,
            format,
        } => commands::extract_audio::extract_audio(&input, &output, &format),
        Commands::Migrate { command } => commands::migrate::migrate(&command),
        Commands::Convert { input, output } => {
            eprintln!("Warning: `convert` is deprecated, use `process` instead.");
            commands::process::process(
                &ProcessArgs {
                    input: &input,
                    output: &output,
                    bitrate: None,
                    crf: None,
                    preset: None,
                    max_fps: None,
                    resize: None,
                    aspect_mode_arg: &AspectModeArg::Fit,
                    crop: None,
                    volume: None,
                    codec: None,
                    audio_codec: None,
                    audio_bitrate: None,
                    h264_profile: None,
                    h264_level: None,
                    allow_color_conversion: false,
                },
                &OutputFormat::Text,
            )
        }
        Commands::Transcode {
            input,
            output,
            bitrate,
            preset,
            max_fps,
            resize,
            aspect_mode,
            format,
        } => {
            eprintln!("Warning: `transcode` is deprecated, use `process` instead.");
            commands::process::process(
                &ProcessArgs {
                    input: &input,
                    output: &output,
                    bitrate: bitrate.as_deref(),
                    crf: None,
                    preset: Some(&preset),
                    max_fps,
                    resize: resize.as_deref(),
                    aspect_mode_arg: &aspect_mode,
                    crop: None,
                    volume: None,
                    codec: None,
                    audio_codec: None,
                    audio_bitrate: None,
                    h264_profile: None,
                    h264_level: None,
                    allow_color_conversion: false,
                },
                &format,
            )
        }
    }
}
