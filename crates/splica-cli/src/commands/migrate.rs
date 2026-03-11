use miette::Result;

// ---------------------------------------------------------------------------
// Migrate command — translates ffmpeg commands to splica equivalents
// ---------------------------------------------------------------------------

/// Parsed result of an ffmpeg command.
struct FfmpegParsed {
    input: Option<String>,
    output: Option<String>,
    resize: Option<String>,
    crop: Option<String>,
    bitrate: Option<String>,
    crf: Option<String>,
    start: Option<String>,
    end: Option<String>,
    no_video: bool,
    volume: Option<String>,
    codec: Option<String>,
    stream_copy: bool,
    unsupported: Vec<String>,
    mappings: Vec<Mapping>,
}

/// A single flag mapping from ffmpeg to splica.
struct Mapping {
    from: String,
    to: String,
}

pub(crate) fn migrate(command: &[String]) -> Result<()> {
    if command.is_empty() {
        return Err(miette::miette!(
            "no ffmpeg command provided\n  \
             → Usage: splica migrate ffmpeg -i input.mp4 output.webm\n  \
             → Or:    splica migrate -i input.mp4 output.webm"
        ));
    }

    let parsed = parse_ffmpeg_args(command);
    let (cmd, explanation) = build_splica_command(&parsed)?;

    println!("{cmd}");
    if !explanation.is_empty() {
        println!();
        println!("Mapped:");
        for line in &explanation {
            println!("  {line}");
        }
    }
    if !parsed.unsupported.is_empty() {
        println!();
        for flag in &parsed.unsupported {
            println!("Warning: splica doesn't support '{flag}' yet");
        }
    }

    Ok(())
}

fn parse_ffmpeg_args(args: &[String]) -> FfmpegParsed {
    let mut parsed = FfmpegParsed {
        input: None,
        output: None,
        resize: None,
        crop: None,
        bitrate: None,
        crf: None,
        start: None,
        end: None,
        no_video: false,
        volume: None,
        codec: None,
        stream_copy: false,
        unsupported: Vec::new(),
        mappings: Vec::new(),
    };

    // Strip leading "ffmpeg" if present.
    let args = if args.first().map(|s| s.as_str()) == Some("ffmpeg") {
        &args[1..]
    } else {
        args
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-i" => {
                if let Some(val) = args.get(i + 1) {
                    parsed.input = Some(val.clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-vf" | "-filter:v" => {
                if let Some(val) = args.get(i + 1) {
                    parse_video_filters(val, &mut parsed);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-af" | "-filter:a" => {
                if let Some(val) = args.get(i + 1) {
                    parse_audio_filters(val, &mut parsed);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-b:v" => {
                if let Some(val) = args.get(i + 1) {
                    parsed.mappings.push(Mapping {
                        from: format!("-b:v {val}"),
                        to: format!("--bitrate {val}"),
                    });
                    parsed.bitrate = Some(val.clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-crf" => {
                if let Some(val) = args.get(i + 1) {
                    parsed.mappings.push(Mapping {
                        from: format!("-crf {val}"),
                        to: format!("--crf {val}"),
                    });
                    parsed.crf = Some(val.clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-ss" => {
                if let Some(val) = args.get(i + 1) {
                    parsed.mappings.push(Mapping {
                        from: format!("-ss {val}"),
                        to: format!("--start {val}"),
                    });
                    parsed.start = Some(val.clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-to" => {
                if let Some(val) = args.get(i + 1) {
                    parsed.mappings.push(Mapping {
                        from: format!("-to {val}"),
                        to: format!("--end {val}"),
                    });
                    parsed.end = Some(val.clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-vn" => {
                parsed.mappings.push(Mapping {
                    from: "-vn".to_string(),
                    to: "extract-audio subcommand".to_string(),
                });
                parsed.no_video = true;
                i += 1;
            }
            "-c:v" | "-vcodec" => {
                if let Some(val) = args.get(i + 1) {
                    if let Some(splica_codec) = map_video_codec(val) {
                        parsed.mappings.push(Mapping {
                            from: format!("-c:v {val}"),
                            to: format!("--codec {splica_codec}"),
                        });
                        parsed.codec = Some(splica_codec.to_string());
                    } else {
                        parsed.unsupported.push(format!("-c:v {val}"));
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-c" | "-codec" => {
                if let Some(val) = args.get(i + 1) {
                    if val == "copy" {
                        parsed.mappings.push(Mapping {
                            from: "-c copy".to_string(),
                            to: "stream copy (no encoding flags)".to_string(),
                        });
                        parsed.stream_copy = true;
                    } else {
                        parsed.unsupported.push(format!("-c {val}"));
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-y" | "-n" | "-hide_banner" | "-loglevel" | "-v" | "-nostdin" => {
                // Skip ffmpeg meta-flags silently (they have no splica equivalent).
                if matches!(arg.as_str(), "-loglevel" | "-v") {
                    i += 2; // these take a value
                } else {
                    i += 1;
                }
            }
            other => {
                // If it starts with '-', it's an unrecognized flag.
                if other.starts_with('-') {
                    // Heuristic: if the next arg doesn't start with '-',
                    // treat it as the flag's value.
                    let flag_with_val = if let Some(next) = args.get(i + 1) {
                        if !next.starts_with('-') {
                            i += 1;
                            format!("{other} {next}")
                        } else {
                            other.to_string()
                        }
                    } else {
                        other.to_string()
                    };
                    parsed.unsupported.push(flag_with_val);
                } else {
                    // Positional argument — treat as output file.
                    parsed.output = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    parsed
}

fn parse_video_filters(filter_str: &str, parsed: &mut FfmpegParsed) {
    for filter in filter_str.split(',') {
        let filter = filter.trim();
        if let Some(params) = filter.strip_prefix("scale=") {
            let splica_resize = params.replace(':', "x");
            parsed.mappings.push(Mapping {
                from: format!("-vf scale={params}"),
                to: format!("--resize {splica_resize}"),
            });
            parsed.resize = Some(splica_resize);
        } else if let Some(params) = filter.strip_prefix("crop=") {
            let parts: Vec<&str> = params.split(':').collect();
            if parts.len() == 4 {
                let splica_crop = format!("{}x{}+{}+{}", parts[0], parts[1], parts[2], parts[3]);
                parsed.mappings.push(Mapping {
                    from: format!("-vf crop={params}"),
                    to: format!("--crop {splica_crop}"),
                });
                parsed.crop = Some(splica_crop);
            } else {
                parsed.unsupported.push(format!("-vf crop={params}"));
            }
        } else {
            parsed.unsupported.push(format!("-vf {filter}"));
        }
    }
}

fn parse_audio_filters(filter_str: &str, parsed: &mut FfmpegParsed) {
    for filter in filter_str.split(',') {
        let filter = filter.trim();
        if let Some(val) = filter.strip_prefix("volume=") {
            parsed.mappings.push(Mapping {
                from: format!("-af volume={val}"),
                to: format!("--volume {val}"),
            });
            parsed.volume = Some(val.to_string());
        } else {
            parsed.unsupported.push(format!("-af {filter}"));
        }
    }
}

fn map_video_codec(ffmpeg_codec: &str) -> Option<&'static str> {
    match ffmpeg_codec {
        "libx264" | "h264" => Some("h264"),
        "libx265" | "hevc" | "h265" => Some("h265"),
        _ => None,
    }
}

fn build_splica_command(parsed: &FfmpegParsed) -> Result<(String, Vec<String>)> {
    let input = parsed.input.as_deref().ok_or_else(|| {
        miette::miette!("no input file found — expected '-i <file>' in the ffmpeg command")
    })?;

    let output = parsed
        .output
        .as_deref()
        .ok_or_else(|| miette::miette!("no output file found in the ffmpeg command"))?;

    let explanation: Vec<String> = parsed
        .mappings
        .iter()
        .map(|m| format!("{}  →  {}", m.from, m.to))
        .collect();

    // Decide which subcommand to use.
    if parsed.no_video {
        let cmd = format!("splica extract-audio --input {input} --output {output}");
        return Ok((cmd, explanation));
    }

    if parsed.start.is_some() || parsed.end.is_some() {
        let mut cmd = format!("splica trim --input {input} --output {output}");
        if let Some(start) = &parsed.start {
            cmd.push_str(&format!(" --start {start}"));
        }
        if let Some(end) = &parsed.end {
            cmd.push_str(&format!(" --end {end}"));
        }
        return Ok((cmd, explanation));
    }

    // Default: process subcommand.
    let mut cmd = format!("splica process --input {input} --output {output}");

    if let Some(resize) = &parsed.resize {
        cmd.push_str(&format!(" --resize {resize}"));
    }
    if let Some(crop) = &parsed.crop {
        cmd.push_str(&format!(" --crop {crop}"));
    }
    if let Some(bitrate) = &parsed.bitrate {
        cmd.push_str(&format!(" --bitrate {bitrate}"));
    }
    if let Some(crf) = &parsed.crf {
        cmd.push_str(&format!(" --crf {crf}"));
    }
    if let Some(volume) = &parsed.volume {
        cmd.push_str(&format!(" --volume {volume}"));
    }
    if let Some(codec) = &parsed.codec {
        cmd.push_str(&format!(" --codec {codec}"));
    }

    Ok((cmd, explanation))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn run(s: &str) -> (FfmpegParsed, String, Vec<String>) {
        let a = args(s);
        let parsed = parse_ffmpeg_args(&a);
        let (cmd, explanation) = build_splica_command(&parsed).unwrap();
        (parsed, cmd, explanation)
    }

    #[test]
    fn test_that_basic_format_conversion_maps_to_process() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 output.webm");

        assert_eq!(cmd, "splica process --input input.mp4 --output output.webm");
    }

    #[test]
    fn test_that_ffmpeg_prefix_is_optional() {
        let (_, cmd, _) = run("-i input.mp4 output.webm");

        assert_eq!(cmd, "splica process --input input.mp4 --output output.webm");
    }

    #[test]
    fn test_that_resize_maps_to_resize_flag() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -vf scale=1280:720 output.mp4");

        assert_eq!(
            cmd,
            "splica process --input input.mp4 --output output.mp4 --resize 1280x720"
        );
    }

    #[test]
    fn test_that_crop_maps_to_crop_flag() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -vf crop=1080:1080:420:0 output.mp4");

        assert_eq!(
            cmd,
            "splica process --input input.mp4 --output output.mp4 --crop 1080x1080+420+0"
        );
    }

    #[test]
    fn test_that_bitrate_maps_to_bitrate_flag() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -b:v 2M output.mp4");

        assert_eq!(
            cmd,
            "splica process --input input.mp4 --output output.mp4 --bitrate 2M"
        );
    }

    #[test]
    fn test_that_crf_maps_to_crf_flag() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -crf 23 output.mp4");

        assert_eq!(
            cmd,
            "splica process --input input.mp4 --output output.mp4 --crf 23"
        );
    }

    #[test]
    fn test_that_trim_maps_to_trim_subcommand() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -ss 1:30 -to 2:00 output.mp4");

        assert_eq!(
            cmd,
            "splica trim --input input.mp4 --output output.mp4 --start 1:30 --end 2:00"
        );
    }

    #[test]
    fn test_that_vn_maps_to_extract_audio() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -vn output.aac");

        assert_eq!(
            cmd,
            "splica extract-audio --input input.mp4 --output output.aac"
        );
    }

    #[test]
    fn test_that_volume_maps_to_volume_flag() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -af volume=0.5 output.mp4");

        assert_eq!(
            cmd,
            "splica process --input input.mp4 --output output.mp4 --volume 0.5"
        );
    }

    #[test]
    fn test_that_codec_libx264_maps_to_h264() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -c:v libx264 output.mp4");

        assert_eq!(
            cmd,
            "splica process --input input.mp4 --output output.mp4 --codec h264"
        );
    }

    #[test]
    fn test_that_codec_libx265_maps_to_h265() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -c:v libx265 output.mp4");

        assert_eq!(
            cmd,
            "splica process --input input.mp4 --output output.mp4 --codec h265"
        );
    }

    #[test]
    fn test_that_stream_copy_maps_to_plain_process() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -c copy output.mkv");

        assert_eq!(cmd, "splica process --input input.mp4 --output output.mkv");
    }

    #[test]
    fn test_that_no_args_returns_error() {
        let result = migrate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_that_unsupported_flags_are_collected() {
        let a = args("ffmpeg -i input.mp4 -r 30 output.mp4");
        let parsed = parse_ffmpeg_args(&a);

        assert_eq!(parsed.unsupported, vec!["-r 30"]);
    }

    #[test]
    fn test_that_chained_video_filters_are_parsed() {
        let (_, cmd, _) =
            run("ffmpeg -i input.mp4 -vf scale=1280:720,crop=1080:720:100:0 output.mp4");

        assert_eq!(
            cmd,
            "splica process --input input.mp4 --output output.mp4 \
             --resize 1280x720 --crop 1080x720+100+0"
        );
    }

    #[test]
    fn test_that_combined_flags_produce_correct_output() {
        let (_, cmd, _) =
            run("ffmpeg -i input.mp4 -c:v libx264 -b:v 2M -vf scale=1280:720 output.mp4");

        assert_eq!(
            cmd,
            "splica process --input input.mp4 --output output.mp4 \
             --resize 1280x720 --bitrate 2M --codec h264"
        );
    }

    #[test]
    fn test_that_missing_input_returns_error() {
        let a = args("ffmpeg output.mp4");
        let parsed = parse_ffmpeg_args(&a);
        let result = build_splica_command(&parsed);

        assert!(result.is_err());
    }

    #[test]
    fn test_that_missing_output_returns_error() {
        let a = args("ffmpeg -i input.mp4");
        let parsed = parse_ffmpeg_args(&a);
        let result = build_splica_command(&parsed);

        assert!(result.is_err());
    }

    #[test]
    fn test_that_unsupported_codec_is_flagged() {
        let a = args("ffmpeg -i input.mp4 -c:v libvpx output.webm");
        let parsed = parse_ffmpeg_args(&a);

        assert_eq!(parsed.unsupported, vec!["-c:v libvpx"]);
    }

    #[test]
    fn test_that_ss_only_maps_to_trim_with_start() {
        let (_, cmd, _) = run("ffmpeg -i input.mp4 -ss 1:30 output.mp4");

        assert_eq!(
            cmd,
            "splica trim --input input.mp4 --output output.mp4 --start 1:30"
        );
    }

    #[test]
    fn test_that_mappings_are_recorded() {
        let (_, _, explanation) = run("ffmpeg -i input.mp4 -vf scale=1280:720 output.mp4");

        assert_eq!(explanation.len(), 1);
        assert_eq!(explanation[0], "-vf scale=1280:720  →  --resize 1280x720");
    }
}
