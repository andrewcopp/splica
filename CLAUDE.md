# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**splica** is a Rust-based media processing library and CLI — a modern alternative to ffmpeg targeting the 90% of production workloads (H.264/H.265/AV1 + AAC/Opus in MP4/WebM/MKV). The name comes from "splice" (film editing) with a Rust-y ending.

### Design Philosophy

- **Memory safety by default** — no unsafe code outside explicitly audited FFI boundaries
- **Sane API over completeness** — cover common codecs/containers well rather than supporting every obscure format
- **WASM-first** — browser-based video processing is a first-class target, not an afterthought
- **Single binary distribution** — `cargo install splica` should just work
- **Typed pipeline API** — composable filter graphs via Rust's type system, not string-based CLI args

## Build & Development Commands

```bash
cargo build                          # Build all crates
cargo build --release                # Release build
cargo test                           # Run all tests
cargo test -p splica-mp4             # Test a single crate
cargo test test_name                 # Run a single test by name
cargo clippy --all-targets           # Lint
cargo fmt --check                    # Check formatting
cargo fmt                            # Auto-format
cargo bench                          # Run benchmarks
```

## Workspace Architecture

splica is a Cargo workspace organized as a media processing pipeline:

```
crates/
  splica-core/       # Shared types: timestamps, media types, error types, traits
  splica-mp4/        # MP4 (ISO BMFF) demuxer and muxer
  splica-webm/       # WebM (Matroska subset) demuxer and muxer
  splica-mkv/        # MKV demuxer and muxer
  splica-codec/      # Codec trait definitions and FFI wrappers (openh264, dav1d, libopus)
  splica-filter/     # Typed filter graph engine (scale, crop, trim, concat, audio mix)
  splica-pipeline/   # High-level pipeline orchestration connecting demux → decode → filter → encode → mux
  splica-cli/        # CLI binary ("splica" command)
```

### Dependency Direction

`cli → pipeline → filter → codec → {mp4, webm, mkv} → core`

Lower crates never import higher crates. Each crate depends on `splica-core` for shared types and traits.

### The Media Pipeline

Every operation flows through five stages, mirroring ffmpeg's internal architecture but with typed boundaries:

1. **Demux** — Read container format, extract compressed packets per stream
2. **Decode** — Decompress packets into raw frames (video: YUV planes, audio: PCM samples)
3. **Filter** — Transform frames (scale, crop, overlay, mix, etc.)
4. **Encode** — Compress frames back into codec packets
5. **Mux** — Write packets into output container format

### Core Traits (in `splica-core`)

- `Demuxer` — reads a container, yields `Packet` streams
- `Decoder` — takes `Packet`, produces `Frame`
- `Filter` — takes `Frame`, produces `Frame` (composable)
- `Encoder` — takes `Frame`, produces `Packet`
- `Muxer` — takes `Packet` streams, writes a container

## Error Handling

Use `thiserror` for library error types and `miette` for CLI error reporting. Follow the error highway pattern:

- Each crate defines its own error enum (e.g., `Mp4Error`, `CodecError`)
- `splica-core` defines `SplicaError` that wraps all crate-level errors
- Errors are explicit via `Result<T, E>` — never panic in library code
- CLI layer converts `SplicaError` into human-readable diagnostics with context

```rust
// Crate-level errors
#[derive(Debug, thiserror::Error)]
pub enum Mp4Error {
    #[error("invalid box type: {0}")]
    InvalidBox(String),
    #[error("unexpected EOF at offset {offset}")]
    UnexpectedEof { offset: u64 },
    #[error("unsupported codec: {0}")]
    UnsupportedCodec(String),
}
```

## Codec Strategy

**Phase 1 (FFI wrappers):** Wrap existing C libraries for codec work:
- Video decode: `dav1d` (AV1), FFI to platform decoders
- Video encode: `openh264` (H.264), `rav1e` (AV1, pure Rust)
- Audio: `libopus` (Opus), `fdk-aac` (AAC)

**Phase 2 (pure Rust):** Incrementally replace FFI with pure Rust implementations where `rav1e` has proven the approach viable.

All FFI code lives in `splica-codec` behind feature flags. `unsafe` blocks are confined to FFI boundary modules and must include `// SAFETY:` comments.

## Testing Conventions

- **Naming:** `test_that_{scenario}` — reads as English, describes the behavior
- **Structure:** GIVEN/WHEN/THEN sections separated by blank lines
- **One assertion per test** — each test has exactly one reason to fail
- **Test fixtures:** Sample media files go in `tests/fixtures/` at the workspace root
- **Deterministic:** Pin timestamps, avoid filesystem ordering assumptions

```rust
#[test]
fn test_that_mp4_demuxer_reads_track_count() {
    // GIVEN
    let data = include_bytes!("../tests/fixtures/sample.mp4");

    // WHEN
    let container = Mp4Demuxer::read(data).unwrap();

    // THEN
    assert_eq!(container.tracks().len(), 2);
}
```

## Design Principles

### Values over objects

Prefer plain data structs and enums over stateful objects with methods. Data flows through functions; it doesn't hide inside objects. When a struct has methods, they should transform or query — not mutate hidden state.

### Pure functions first

Separate pure computation from side effects. Functions that parse, validate, transform, or decide should take inputs and return outputs with no I/O. Push I/O (file reads, network, logging) to the outer edges — CLI entry points, pipeline orchestration, FFI boundaries.

### Immutable by default

Rust enforces this at the language level — lean into it. Don't take `&mut self` when `&self` works. Don't use interior mutability (`Cell`, `RefCell`, `Mutex`) unless concurrency or FFI genuinely requires it. Prefer returning new values over mutating existing ones.

### Make invalid states unrepresentable

Use the type system to eliminate error categories entirely. Newtypes for domain concepts (`TrackIndex(u32)` not bare `u32`). Enums for closed sets of options. Builder patterns that enforce required fields at compile time. If a function can't receive bad input, it doesn't need to validate it.

### Composition over inheritance

Rust has no inheritance — use that as a feature, not a limitation. Compose behavior via:
- Traits for shared interfaces (`Demuxer`, `Filter`, `Encoder`)
- Generics for static dispatch in hot paths
- Trait objects (`dyn Filter`) for dynamic dispatch in user-facing APIs like filter graphs
- Wrapper types for extending behavior (not trait inheritance hierarchies)

### Dependency injection via traits

Concrete types depend on trait bounds, not other concrete types. This is how we keep crates decoupled and tests simple:

```rust
// Good: codec crate doesn't know about mp4 crate
fn transcode<D: Demuxer, E: Encoder>(demuxer: &mut D, encoder: &mut E) -> Result<()>

// Bad: hardcoded dependency
fn transcode(demuxer: &mut Mp4Demuxer, encoder: &mut H264Encoder) -> Result<()>
```

### Small, focused types

Many small structs that each do one thing, not a few god structs. A `TrackInfo` holds metadata. A `Packet` holds compressed data. A `Timestamp` handles time math. They don't know about each other's internals.

### Explicit over implicit

No global mutable state. No lazy_static configuration. No implicit initialization. If a function needs a codec registry, it takes one as a parameter. If a pipeline needs a thread pool, it's passed in at construction.

## Code Conventions

- **No `unwrap()` in library code** — use `?` or explicit error handling
- **Builder pattern for complex construction** — encoders, pipelines, filter graphs
- **Feature flags for optional codecs** — users opt into codec dependencies they need
- **`#[must_use]`** on types where ignoring the return value is always a bug
- **Keep `unsafe` minimal** — only in FFI glue, always with safety comments
