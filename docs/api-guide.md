# splica API Guide

This guide walks through using splica as a Rust library for media processing.
All examples assume you are building a Rust application that depends on splica
crates.

## Adding splica as a dependency

splica is split into focused crates. Add only what you need:

```toml
[dependencies]
# Core types (TrackIndex, Timestamp, traits) — always required
splica-core = "0.1"

# Container demuxers/muxers — pick the formats you need
splica-mp4 = "0.1"
splica-webm = "0.1"
splica-mkv = "0.1"

# Codec wrappers — enable via feature flags
splica-codec = { version = "0.1", features = ["h264", "h265", "opus"] }

# Filters (scale, crop, volume, etc.)
splica-filter = "0.1"

# High-level pipeline orchestration
splica-pipeline = "0.1"
```

Feature flags on `splica-codec` control which FFI codec libraries are compiled:

| Flag   | Codec        | Underlying library |
|--------|--------------|--------------------|
| `h264` | H.264        | openh264           |
| `h265` | H.265/HEVC   | kvazaar            |
| `av1`  | AV1          | dav1d / rav1e      |
| `aac`  | AAC          | fdk-aac            |
| `opus` | Opus         | libopus            |

Only enabled codecs are linked, keeping binary size minimal.

## Basic transcode: file to file

The most common use case is reading a media file, processing it, and writing
the result:

```rust,ignore
use std::fs::File;
use std::io::{BufReader, BufWriter};

use splica_core::{Codec, TrackIndex, VideoCodec};
use splica_codec::{H264Decoder, H265Encoder};
use splica_mp4::{Mp4Demuxer, Mp4Muxer};
use splica_pipeline::PipelineBuilder;

fn transcode_h264_to_h265(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Open input
    let reader = BufReader::new(File::open(input)?);
    let demuxer = Mp4Demuxer::open(reader)?;

    // Open output
    let writer = BufWriter::new(File::create(output)?);
    let muxer = Mp4Muxer::new(writer);

    // Configure transcode for video track 0
    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_decoder(TrackIndex(0), H264Decoder::new())
        .with_encoder(TrackIndex(0), H265Encoder::new())
        .with_output_codec(TrackIndex(0), Codec::Video(VideoCodec::H265))
        .with_muxer(muxer)
        .build()?;

    pipeline.run()?;
    Ok(())
}
```

**Key points:**

- Tracks without a decoder/encoder pair are passed through in copy mode
  (compressed packets go directly from demuxer to muxer, no re-encoding).
- `with_output_codec()` tells the muxer what codec to write into container
  metadata. Without it, the muxer uses the codec from the input track.

## Copy mode (remuxing)

To change containers without re-encoding (e.g., MP4 to MKV), skip the
decoder/encoder entirely:

```rust,ignore
use splica_mp4::Mp4Demuxer;
use splica_mkv::MkvMuxer;
use splica_pipeline::PipelineBuilder;

fn remux_mp4_to_mkv(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let demuxer = Mp4Demuxer::open(BufReader::new(File::open(input)?))?;
    let muxer = MkvMuxer::new(BufWriter::new(File::create(output)?));

    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(muxer)
        .build()?;

    pipeline.run()?;
    Ok(())
}
```

All tracks pass through untouched. This is the fastest mode since no
decoding or encoding occurs.

## Custom I/O: in-memory processing

splica's trait-based design means any `Read + Seek` source or `Write`
destination works. Use `Cursor<Vec<u8>>` for in-memory processing:

```rust,ignore
use std::io::Cursor;

use splica_mp4::{Mp4Demuxer, Mp4Muxer};
use splica_pipeline::PipelineBuilder;

fn process_in_memory(input_bytes: Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let demuxer = Mp4Demuxer::open(Cursor::new(input_bytes))?;

    let output = Vec::new();
    let muxer = Mp4Muxer::new(Cursor::new(output));

    let mut pipeline = PipelineBuilder::new()
        .with_demuxer(demuxer)
        .with_muxer(muxer)
        .build()?;

    pipeline.run()?;

    // Retrieve the written bytes from the muxer's inner writer
    // (exact API depends on the muxer implementation)
    Ok(vec![]) // placeholder
}
```

This pattern is especially useful for WASM targets where filesystem access
is unavailable.

## Filter chains

Filters transform frames between decode and encode. They compose by
chaining multiple `with_filter()` calls on the same track. Filters execute
in the order they are added.

```rust,ignore
use splica_core::TrackIndex;
use splica_filter::{ScaleFilter, CropFilter};
use splica_pipeline::PipelineBuilder;

// Scale down to 1280x720, then crop a 1080x620 region starting at (100, 50)
let mut pipeline = PipelineBuilder::new()
    .with_demuxer(demuxer)
    .with_decoder(TrackIndex(0), decoder)
    .with_filter(TrackIndex(0), ScaleFilter::new(1280, 720))
    .with_filter(TrackIndex(0), CropFilter::new(100, 50, 1080, 620))
    .with_encoder(TrackIndex(0), encoder)
    .with_output_dimensions(TrackIndex(0), 1080, 620)
    .with_muxer(muxer)
    .build()?;

pipeline.run()?;
```

**Audio filters** work the same way via `with_audio_filter()`:

```rust,ignore
use splica_filter::VolumeFilter;

let mut pipeline = PipelineBuilder::new()
    .with_demuxer(demuxer)
    .with_audio_decoder(TrackIndex(1), aac_decoder)
    .with_audio_filter(TrackIndex(1), VolumeFilter::new(0.5)) // 50% volume
    .with_audio_encoder(TrackIndex(1), opus_encoder)
    .with_muxer(muxer)
    .build()?;
```

When a filter changes the output dimensions (e.g., scale or crop), call
`with_output_dimensions()` so the muxer writes correct container metadata.

## Progress reporting via event handler

The pipeline emits structured events during execution. Use
`with_event_handler()` to track progress, build progress bars, or collect
metrics.

```rust,ignore
use splica_pipeline::{PipelineBuilder, PipelineEvent, PipelineEventKind};
use std::sync::{Arc, Mutex};

let progress = Arc::new(Mutex::new(0u64));
let progress_clone = Arc::clone(&progress);

let mut pipeline = PipelineBuilder::new()
    .with_event_handler(move |event: PipelineEvent| {
        match event.kind {
            PipelineEventKind::PacketsRead { count } => {
                *progress_clone.lock().unwrap() = count;
            }
            PipelineEventKind::FramesDecoded { count } => {
                eprintln!("Decoded {} frames", count);
            }
            PipelineEventKind::FramesEncoded { count } => {
                eprintln!("Encoded {} frames", count);
            }
            PipelineEventKind::PacketsWritten { count } => {
                eprintln!("Written {} packets", count);
            }
            _ => {} // PipelineEventKind is #[non_exhaustive]
        }
    })
    .with_demuxer(demuxer)
    .with_muxer(muxer)
    .build()?;

pipeline.run()?;
println!("Total packets read: {}", *progress.lock().unwrap());
```

**Important details:**

- Event counts are **cumulative**, not deltas. The latest `count` value is
  the total so far.
- Each event carries a monotonic `timestamp` (`std::time::Instant`) for
  rate calculations.
- The handler runs **synchronously** on the pipeline thread. Keep it fast.
- `PipelineEventKind` is `#[non_exhaustive]` -- always include a wildcard
  arm so new event types do not break your code.

## Pre-flight validation

Before building, you can check for configuration errors without committing
to execution:

```rust,ignore
use splica_pipeline::PipelineBuilder;

let builder = PipelineBuilder::new()
    .with_demuxer(demuxer)
    .with_muxer(muxer);

let errors = builder.validate();
if !errors.is_empty() {
    for error in &errors {
        eprintln!("Validation error: {error}");
    }
    // Handle errors before calling build()
}
```

`validate()` returns **all** errors at once, unlike `build()` which returns
only the first. Use `validate()` when you want to present a complete list
of issues to the user.

## Error handling patterns

splica uses typed errors throughout. Each pipeline stage has its own error
type, and `PipelineError` wraps them all:

```rust,ignore
use splica_core::PipelineError;

match pipeline.run() {
    Ok(()) => println!("Done"),
    Err(PipelineError::Demux(e)) => {
        eprintln!("Input read failed: {e}");
        if e.kind().is_retryable() {
            // I/O errors like connection reset can be retried
        }
    }
    Err(PipelineError::Decode(e)) => eprintln!("Decode failed: {e}"),
    Err(PipelineError::Filter(e)) => eprintln!("Filter failed: {e}"),
    Err(PipelineError::Encode(e)) => eprintln!("Encode failed: {e}"),
    Err(PipelineError::Mux(e)) => eprintln!("Output write failed: {e}"),
    Err(PipelineError::Validation(e)) => eprintln!("Config error: {e}"),
    Err(PipelineError::Config { message }) => eprintln!("Config error: {message}"),
    Err(e) => eprintln!("Pipeline error: {e}"),
}
```

**Design principles:**

- Library code never panics. All errors are returned via `Result`.
- `DemuxError` exposes an `ErrorKind` with `is_retryable()` for I/O
  failure recovery.
- `PipelineError::Validation` wraps `ValidationError`, which enumerates
  specific configuration mistakes (missing demuxer, orphan filters, etc.).

## Frame rate limiting

To reduce the output frame rate (e.g., converting 60fps to 30fps), use
`with_max_fps()`:

```rust,ignore
use splica_core::TrackIndex;

let mut pipeline = PipelineBuilder::new()
    .with_demuxer(demuxer)
    .with_decoder(TrackIndex(0), decoder)
    .with_encoder(TrackIndex(0), encoder)
    .with_max_fps(TrackIndex(0), 30.0)
    .with_muxer(muxer)
    .build()?;
```

Frames closer together than `1/max_fps` seconds are dropped before
filtering and encoding, saving CPU on both the filter and encode stages.

## Implementing custom traits

splica's pipeline accepts any implementation of its core traits. This is
how you integrate custom container formats or codecs:

```rust,ignore
use splica_core::{Demuxer, DemuxError, Packet, TrackInfo};

struct MyDemuxer<R> {
    reader: R,
    tracks: Vec<TrackInfo>,
}

impl<R: std::io::Read + std::io::Seek> Demuxer for MyDemuxer<R> {
    fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    fn read_packet(&mut self) -> Result<Option<Packet>, DemuxError> {
        // Read and parse your container format here
        Ok(None) // placeholder
    }
}
```

The same pattern applies to `Decoder`, `Encoder`, `Muxer`, `VideoFilter`,
`AudioFilter`, and their audio variants. All traits are object-safe, so
they work as `Box<dyn Demuxer>` in generic pipeline code.
