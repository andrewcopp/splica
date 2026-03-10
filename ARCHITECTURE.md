# Architecture

This document describes the technical architecture of splica. It covers the pipeline model, crate structure, key design decisions, and the constraints that shape them.

## The Pipeline Model

All media processing is a pipeline of five stages:

```
┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐
│  Demux  │───▶│ Decode  │───▶│ Filter  │───▶│ Encode  │───▶│   Mux   │
│         │    │         │    │         │    │         │    │         │
│ Container│    │ Codec   │    │ Transform│   │ Codec   │    │Container│
│ → Packets│    │→ Frames │    │→ Frames │    │→ Packets│    │→ Output │
└─────────┘    └─────────┘    └─────────┘    └─────────┘    └─────────┘
```

**Packets** are compressed data belonging to a single stream (video, audio, subtitle). They carry timestamps (PTS/DTS) and codec-specific metadata.

**Frames** are raw, uncompressed data — YUV pixel planes for video, PCM samples for audio. Filters operate exclusively on frames.

This distinction drives the crate boundaries: container crates deal in packets, codec crates convert between packets and frames, filter crates transform frames.

## Crate Structure

```
crates/
  splica-core/       # Types and traits shared across all crates
  splica-mp4/        # MP4 demuxer/muxer
  splica-webm/       # WebM demuxer/muxer
  splica-mkv/        # MKV demuxer/muxer
  splica-codec/      # Codec trait impls + FFI wrappers
  splica-filter/     # Filter graph engine
  splica-pipeline/   # High-level orchestration
  splica-cli/        # Binary
```

### Dependency graph

```
splica-cli
    └── splica-pipeline
            ├── splica-filter
            │       └── splica-core
            ├── splica-codec
            │       └── splica-core
            ├── splica-mp4
            │       └── splica-core
            ├── splica-webm
            │       └── splica-core
            └── splica-mkv
                    └── splica-core
```

The rule: **dependencies only point down and toward core.** A container crate never imports a codec. A codec never imports a filter. `splica-pipeline` is the integration point that wires everything together.

## splica-core: The Shared Foundation

Defines the types and traits that all other crates depend on:

### Media Types

```rust
/// Compressed data from a single stream
struct Packet {
    track_index: u32,
    pts: Timestamp,         // Presentation timestamp
    dts: Option<Timestamp>, // Decode timestamp (when B-frames are present)
    keyframe: bool,
    data: Vec<u8>,
}

/// Raw decoded media
enum Frame {
    Video(VideoFrame),  // YUV planes, resolution, pixel format
    Audio(AudioFrame),  // PCM samples, sample rate, channel layout
}

/// Rational timestamp in stream timebase
struct Timestamp {
    pts: i64,
    timebase: Rational, // e.g., 1/90000 for video, 1/48000 for audio
}
```

### Core Traits

```rust
trait Demuxer {
    fn tracks(&self) -> &[TrackInfo];
    fn read_packet(&mut self) -> Result<Option<Packet>>;
    fn seek(&mut self, target: Timestamp) -> Result<()>;
}

trait Decoder {
    fn send_packet(&mut self, packet: &Packet) -> Result<()>;
    fn receive_frame(&mut self) -> Result<Option<Frame>>;
}

trait Filter {
    fn process(&mut self, frame: Frame) -> Result<Option<Frame>>;
    fn flush(&mut self) -> Result<Vec<Frame>>;
}

trait Encoder {
    fn send_frame(&mut self, frame: &Frame) -> Result<()>;
    fn receive_packet(&mut self) -> Result<Option<Packet>>;
}

trait Muxer {
    fn add_track(&mut self, info: TrackInfo) -> Result<u32>;
    fn write_packet(&mut self, packet: &Packet) -> Result<()>;
    fn finalize(&mut self) -> Result<()>;
}
```

The `send/receive` pattern for codecs mirrors the async nature of codec pipelines — you may send multiple packets before receiving a frame (decoder buffering) or vice versa (encoder lookahead). This is how real codecs work and ffmpeg uses the same pattern internally, but splica makes it explicit in the type system.

## Container Crates (mp4, webm, mkv)

Each container crate implements `Demuxer` and `Muxer` for its format. Container crates:

- Parse binary container structures into typed representations
- Handle seeking, timestamp remapping, and track indexing
- Know nothing about codec internals — they pass opaque `Packet` data

### MP4 (ISO BMFF)

MP4 is a box-based format. Every structure is a typed box with a length prefix:

```
[ftyp] File type
[moov] Movie metadata
  [mvhd] Movie header (duration, timescale)
  [trak] Track (one per stream)
    [tkhd] Track header
    [mdia] Media
      [mdhd] Media header (stream timescale)
      [hdlr] Handler (video/audio/subtitle)
      [minf] Media info
        [stbl] Sample table
          [stsd] Sample descriptions (codec config)
          [stts] Time-to-sample (frame durations)
          [stsc] Sample-to-chunk mapping
          [stco] Chunk offsets (byte positions in file)
          [stsz] Sample sizes
          [stss] Sync samples (keyframes)
[mdat] Media data (raw packets, referenced by stbl)
```

The demuxer reads `moov` to build an index, then reads packets from `mdat` on demand. The muxer builds `stbl` tables incrementally and writes `moov` at the end (or at the start for streaming-optimized files via `moov` atom relocation).

### WebM / MKV

Both use the EBML (Extensible Binary Meta Language) format — a binary XML-like structure with nested elements. WebM is a restricted subset of MKV (only VP8/VP9/AV1 video and Vorbis/Opus audio). The crates share EBML parsing infrastructure.

## splica-codec: Codec Wrappers

Provides `Decoder` and `Encoder` implementations. Codecs are behind feature flags — users only compile what they need.

### FFI Strategy

Most codecs wrap C libraries:

```
splica-codec/
  src/
    h264/
      mod.rs          # Public Decoder/Encoder types
      ffi.rs          # Raw C bindings (unsafe, minimal)
      decoder.rs      # Safe wrapper implementing Decoder trait
      encoder.rs      # Safe wrapper implementing Encoder trait
    av1/
      decode_ffi.rs   # dav1d bindings
      encode.rs       # rav1e (pure Rust, no FFI)
    opus/
      ffi.rs
      decoder.rs
      encoder.rs
```

FFI boundary rules:
- All `unsafe` is in `ffi.rs` files
- Every `unsafe` block has a `// SAFETY:` comment explaining the invariant
- Safe wrappers own the C resources and implement `Drop` for cleanup
- C pointers never escape the wrapper type's methods

### Feature flags

```toml
[features]
default = ["h264", "aac"]
full = ["h264", "h265", "av1", "aac", "opus"]
h264 = ["dep:openh264-sys"]
h265 = []  # planned
av1 = ["dep:dav1d-sys", "dep:rav1e"]
aac = ["dep:fdk-aac-sys"]
opus = ["dep:libopus-sys"]
```

## splica-filter: The Filter Graph

Filters transform frames. The filter graph is a DAG (directed acyclic graph) of filter nodes, but for the common case, a linear chain is exposed via a simpler API.

### Linear chain (common case)

```rust
let chain = FilterChain::new()
    .then(Scale::new(1280, 720))
    .then(Crop::new(0, 0, 1280, 600))
    .build()?;

let output = chain.process(frame)?;
```

### Filter graph (advanced)

```rust
let mut graph = FilterGraph::new();
let input = graph.add_input(stream_info);
let scaled = graph.add(Scale::new(1280, 720), &[input]);
let overlay_input = graph.add_input(overlay_info);
let composited = graph.add(Overlay::new(10, 10), &[scaled, overlay_input]);
let output = graph.add_output(composited);
graph.build()?;
```

The graph validates at build time:
- No cycles
- Compatible pixel formats between connected nodes (or auto-inserts conversion)
- Audio sample rates match (or auto-inserts resampler)

### Built-in filters

| Filter | Domain | Description |
|--------|--------|-------------|
| Scale | Video | Resize with configurable algorithm (bilinear, lanczos, etc.) |
| Crop | Video | Crop to rectangle |
| Trim | Both | Select time range |
| Concat | Both | Join streams sequentially |
| Overlay | Video | Composite one video on top of another |
| Volume | Audio | Adjust gain |
| Mix | Audio | Mix multiple audio streams |
| Resample | Audio | Convert sample rate |

## splica-pipeline: Orchestration

The pipeline connects all stages. It handles:

- **Stream mapping** — which input tracks connect to which outputs
- **Timestamp management** — converting between stream timebases
- **Threading** — decode/filter/encode can run on separate threads with bounded channels
- **Progress reporting** — callbacks for frame count, elapsed time, ETA
- **Resource cleanup** — everything is dropped in the right order via ownership

### Threading model

```
Thread 1 (demux):     read packets → send to decoder channels
Thread 2 (decode):    receive packets → decode → send frames to filter
Thread 3 (filter):    receive frames → filter → send to encoder
Thread 4 (encode):    receive frames → encode → send packets to muxer
Thread 5 (mux):       receive packets → write output
```

Channels are bounded to prevent memory blowup on fast-demux/slow-encode workloads. Back-pressure propagates naturally through full channels.

## WASM Considerations

WASM is a first-class target, which creates constraints:

- **No filesystem by default** — APIs accept `Read + Seek` / `Write` traits, not file paths
- **No threads in base WASM** — pipeline must work single-threaded (threads available with `wasm32-unknown-unknown` + SharedArrayBuffer)
- **No FFI in WASM** — codec implementations must be pure Rust or compiled-to-WASM C (via wasm32-unknown-emscripten). Feature flags control this.
- **Memory pressure** — WASM has a single linear memory. Streaming processing is essential; never buffer an entire file.

The WASM build strips FFI codecs and uses pure-Rust alternatives where available. `rav1e` already compiles to WASM. Pure-Rust H.264 decode is a longer-term goal.

## Error Design

Each crate has its own error enum. Errors compose upward:

```
Mp4Error ─┐
WebmError ─┤
MkvError ──┼──▶ SplicaError (in splica-core)──▶ miette diagnostic (in splica-cli)
CodecError ┤
FilterError┘
```

Library consumers get structured `SplicaError` with context. CLI users get formatted diagnostics with suggestions. The error type carries enough information to produce both.

### Error categories

- **Format errors** — malformed container data, invalid box sizes, truncated files
- **Codec errors** — unsupported profiles, decode failures, encoder configuration errors
- **Pipeline errors** — incompatible stream configurations, missing codecs for requested format
- **I/O errors** — wrapping `std::io::Error` with file path context

## Performance Strategy

Performance priorities, in order:

1. **I/O efficiency** — minimize copies, use buffered readers, support memory-mapped input
2. **Parallelism** — pipeline stages run concurrently with bounded channels
3. **SIMD** — use `std::simd` (when stable) for pixel format conversion and filter kernels
4. **Hardware acceleration** — expose platform video APIs (VideoToolbox, VAAPI, NVENC) behind feature flags for encode/decode, falling back to software codecs

Raw software codec speed will not match ffmpeg's hand-tuned Assembly on day one. That's acceptable — most production pipelines use hardware encoders or are I/O-bound.
