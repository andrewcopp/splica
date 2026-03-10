# splica

A modern media processing library and CLI, built in Rust. Covers the 90% of video/audio workloads that matter — with memory safety, a sane API, and single-binary distribution.

## Why

ffmpeg is indispensable. It's also a C codebase from 2000 with a cryptic CLI, an unsafe API, and a constant stream of memory corruption CVEs. Every service that accepts user-uploaded video is exposed.

splica takes a different approach:

| | ffmpeg | splica |
|---|---|---|
| **Language** | C + hand-written Assembly | Rust (unsafe confined to FFI boundaries) |
| **CLI** | Cryptic flag soup | Structured subcommands with helpful errors |
| **Library API** | Opaque structs, negative int error codes, manual lifecycle management | Typed builders, `Result<T, E>`, ownership-driven resource management |
| **Distribution** | Shared library hell | `cargo install splica` |
| **Browser** | Painful WASM port | First-class WASM target |
| **Format coverage** | Everything ever invented | H.264, H.265, AV1, AAC, Opus in MP4, WebM, MKV |

splica does not aim to replace ffmpeg for every use case. If you need to transcode Sega Dreamcast video captures or mux RealMedia streams, ffmpeg is your tool. If you're building a product that processes video in 2026, splica is.

## CLI

```bash
# Transcode to H.265, keep audio
splica transcode input.mp4 -c:v h265 -o output.mp4

# Extract audio as Opus
splica extract audio input.mp4 -c opus -o output.ogg

# Scale and trim
splica transcode input.mp4 \
  --scale 1280x720 \
  --trim 00:01:30..00:04:00 \
  -o output.mp4

# Probe container info
splica probe input.mp4

# Concatenate multiple files
splica concat part1.mp4 part2.mp4 part3.mp4 -o full.mp4

# HLS packaging for streaming
splica package input.mp4 --format hls --segment-duration 6 -o output/
```

Errors tell you what went wrong and how to fix it:

```
Error: Unsupported codec 'vp8' for container 'mp4'

  MP4 containers support: H.264, H.265, AV1
  VP8 is supported in: WebM

  Try: splica transcode input.mp4 -c:v vp8 -o output.webm
```

## Library API

### Basic transcoding

```rust
use splica::{Pipeline, Codec, Container};

let pipeline = Pipeline::open("input.mp4")?
    .video(|v| v.codec(Codec::H265).bitrate("4M"))
    .audio(|a| a.codec(Codec::Opus).bitrate("128k"))
    .output(Container::Mp4)
    .build()?;

pipeline.run("output.mp4")?;
```

### Filter graph

```rust
use splica::{Pipeline, filters};

let pipeline = Pipeline::open("input.mp4")?
    .video(|v| {
        v.filter(filters::Scale::new(1280, 720))
         .filter(filters::Trim::new("00:01:30".."00:04:00"))
    })
    .build()?;

pipeline.run("output.mp4")?;
```

### Probing

```rust
use splica::probe;

let info = probe("input.mp4")?;
println!("Duration: {}", info.duration());
for track in info.tracks() {
    println!("  {} — {} {}", track.index(), track.codec(), track.kind());
}
```

### Low-level: reading packets

```rust
use splica::mp4::Mp4Demuxer;

let mut demuxer = Mp4Demuxer::open("input.mp4")?;
for track in demuxer.tracks() {
    println!("Track {}: {} {:?}", track.index(), track.codec(), track.resolution());
}

while let Some(packet) = demuxer.read_packet()? {
    println!("Track {} | pts={} | {} bytes", packet.track, packet.pts, packet.data.len());
}
```

## Supported Formats

### Containers
| Format | Demux | Mux |
|--------|-------|-----|
| MP4 (ISO BMFF) | Yes | Yes |
| WebM | Yes | Yes |
| MKV | Yes | Yes |

### Video Codecs
| Codec | Decode | Encode | Backend |
|-------|--------|--------|---------|
| H.264 | Yes | Yes | openh264 (FFI) |
| H.265/HEVC | Yes | Planned | FFI |
| AV1 | Yes | Yes | dav1d (decode, FFI), rav1e (encode, pure Rust) |

### Audio Codecs
| Codec | Decode | Encode | Backend |
|-------|--------|--------|---------|
| AAC | Yes | Yes | fdk-aac (FFI) |
| Opus | Yes | Yes | libopus (FFI) |

## Building from Source

```bash
git clone https://github.com/TODO/splica.git
cd splica
cargo build --release
```

The binary is at `target/release/splica`.

### Feature Flags

```bash
# Minimal build (MP4 demux only, no codecs)
cargo build --no-default-features

# Full build with all codecs
cargo build --features full

# Specific codecs
cargo build --features "h264,av1,opus"
```

## Project Status

splica is in early development. The API is unstable and will change.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
