# CLAUDE.md

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
git config core.hooksPath .githooks  # Enable hooks (one-time setup per clone)
```

### Pre-commit Checklist

**Before every commit**, run all three in order:
```bash
cargo fmt                                        # Fix formatting
cargo clippy --all-targets -- -D warnings        # Zero warnings allowed
cargo test                                       # All tests pass
```

The pre-commit hook enforces fmt + clippy. The pre-push hook enforces all three. Enable hooks at the start of every session: `git config core.hooksPath .githooks`

## Workspace Architecture

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

## Error Handling

Use `thiserror` for library error types and `miette` for CLI error reporting. Each crate defines its own error enum (e.g., `Mp4Error`, `CodecError`). `splica-core` defines `SplicaError` that wraps all crate-level errors. Never panic in library code. CLI layer converts `SplicaError` into human-readable diagnostics with context.

## Testing Conventions

- **Naming:** `test_that_{scenario}` — reads as English, describes the behavior
- **Structure:** GIVEN/WHEN/THEN sections separated by blank lines
- **One assertion per test** — each test has exactly one reason to fail
- **Test fixtures:** Sample media files go in `tests/fixtures/` at the workspace root
- **Deterministic:** Pin timestamps, avoid filesystem ordering assumptions

## Design Principles

- **Values over objects** — prefer plain data structs and enums over stateful objects with methods. Data flows through functions; it doesn't hide inside objects.
- **Make invalid states unrepresentable** — newtypes for domain concepts (`TrackIndex(u32)` not bare `u32`), enums for closed sets, builders that enforce required fields at compile time.
- **Dependency injection via traits** — concrete types depend on trait bounds, not other concrete types. This keeps crates decoupled and tests simple.
- **Small, focused types** — many small structs that each do one thing, not a few god structs.
- **Size is a smell** — a file over 300 lines or a function over 30 lines is a signal to split. Treat these as triggers to rethink structure, not hard limits.

## Sprint Cadence

**3:1 model with trigger override.** Three feature sprints, then one dedicated tech debt sprint — but don't wait for the calendar if the trigger fires early.

A debt sprint fires on whichever comes first:
1. Three feature sprints have elapsed since the last debt sprint, **OR**
2. Any single file crosses **500 lines** of non-test code

### Debt Sprint Rules
- Dana (tech-lead) runs an end-of-sprint review at the close of every sprint and updates the debt register.
- During a debt sprint, the acceptance criteria are: every file over 500 lines gets under 500 lines, all tests pass, no behavior changes, Dana signs off.
- P0-severity debt items (silent wrong output, correctness risks) get scheduled into the next sprint automatically, regardless of cadence.
- Medium-severity items cannot be carried more than two sprints without an explicit priority call.

## Work Modes

Every change falls into exactly one of three modes. Never mix modes in a single commit.

- **`feat:`** — Adding or changing functionality. The diff should be about *what the code does*.
- **`fix:`** — Correcting broken behavior. The diff should be about *making existing behavior correct*.
- **`refactor:`** — Improving structure without changing behavior. Tests should pass before and after with the same results.

### Rules

- **One mode per commit.** If a diff touches both behavior and structure, it's two commits.
- **One task per PR.** A PR may contain multiple commits across modes, but they all serve the same task.
- **Discovery is expected.** When working on a feature or fix, you'll often discover a refactor is needed first. Stop, do the refactor, commit it, then continue with the original work.
- **Refactors are first-class work.** They get their own commits, their own review consideration, and their own justification.

## Code Conventions

- **No `unwrap()` in library code** — use `?` or explicit error handling
- **Builder pattern for complex construction** — encoders, pipelines, filter graphs
- **Feature flags for optional codecs** — users opt into codec dependencies they need
- **`#[must_use]`** on types where ignoring the return value is always a bug
- **Keep `unsafe` minimal** — only in FFI glue, always with `// SAFETY:` comments
- **All FFI code lives in `splica-codec`** behind feature flags
