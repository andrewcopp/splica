mod args;
mod reencode;
mod stream_copy;
mod wiring;

use miette::Result;

pub(crate) use args::ProcessArgs;

use super::{
    classify_error, output_container, validate_output_format, CompleteEvent, ErrorResult,
    OutputFormat, TranscodeAudioInfo,
};

struct TranscodeOutput {
    packets_read: u64,
    frames_decoded: u64,
    frames_encoded: u64,
    packets_written: u64,
    audio_tracks: Vec<TranscodeAudioInfo>,
    mux_ok: bool,
    output_codec: Option<String>,
    output_duration_secs: Option<f64>,
    output_bitrate_kbps: Option<u64>,
}

pub(crate) fn process(args: &ProcessArgs<'_>, format: &OutputFormat) -> Result<()> {
    let json_mode = matches!(format, OutputFormat::Json);

    // In JSON mode, catch errors and emit structured JSON instead of miette output
    let result = process_inner(args, json_mode);

    if json_mode {
        match result {
            Ok(out) => {
                let complete = CompleteEvent {
                    event_type: "complete",
                    input: args.input.display().to_string(),
                    output: args.output.display().to_string(),
                    packets_read: out.packets_read,
                    frames_decoded: out.frames_decoded,
                    frames_encoded: out.frames_encoded,
                    packets_written: out.packets_written,
                    audio_tracks: out.audio_tracks,
                    mux_ok: out.mux_ok,
                    output_codec: out.output_codec,
                    output_duration_secs: out.output_duration_secs,
                    output_bitrate_kbps: out.output_bitrate_kbps,
                };
                println!("{}", serde_json::to_string(&complete).unwrap());
                Ok(())
            }
            Err(e) => {
                let (error_kind, code) = classify_error(&e);
                let error_json = ErrorResult {
                    event_type: "error",
                    error_kind: error_kind.to_string(),
                    message: format!("{e}"),
                };
                println!("{}", serde_json::to_string(&error_json).unwrap());
                std::process::exit(code);
            }
        }
    } else {
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

fn process_inner(args: &ProcessArgs<'_>, json_mode: bool) -> Result<TranscodeOutput> {
    validate_output_format(args.output)?;

    // Determine if user explicitly requested re-encoding via any encoding option
    let user_requested_reencode = args.bitrate.is_some()
        || args.crf.is_some()
        || args.preset.is_some()
        || args.max_fps.is_some()
        || args.resize.is_some()
        || args.crop.is_some()
        || args.volume.is_some()
        || args.codec.is_some();

    if !user_requested_reencode {
        // Check if stream copy is possible (all codecs compatible, no audio transcode needed)
        let out_container =
            output_container(args.output).unwrap_or(splica_core::ContainerFormat::Mp4);

        let demuxer = super::open_demuxer(args.input)?;
        let tracks = demuxer.tracks().to_vec();

        let video_needs_reencode = tracks
            .iter()
            .filter(|t| t.kind == splica_core::TrackKind::Video)
            .any(|t| !reencode::is_video_codec_compatible(&t.codec, out_container));

        let target_audio_codec = match out_container {
            splica_core::ContainerFormat::WebM | splica_core::ContainerFormat::Mkv => {
                splica_core::AudioCodec::Opus
            }
            splica_core::ContainerFormat::Mp4 => splica_core::AudioCodec::Aac,
        };

        let any_audio_needs_transcode = tracks
            .iter()
            .filter(|t| t.kind == splica_core::TrackKind::Audio)
            .any(|t| {
                if let splica_core::Codec::Audio(ref ac) = t.codec {
                    *ac != target_audio_codec
                } else {
                    false
                }
            });

        if !video_needs_reencode && !any_audio_needs_transcode {
            return stream_copy::stream_copy(args, json_mode);
        }
    }

    reencode::reencode(args, json_mode)
}
