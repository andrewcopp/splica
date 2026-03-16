# Tech Lead Memory

## Codebase Overview

splica is a Rust-based media processing library and CLI. Workspace crates:
`splica-core` → `splica-mp4`, `splica-webm`, `splica-mkv` → `splica-codec` → `splica-filter` → `splica-pipeline` → `splica-cli`.

Key architectural decisions confirmed:
- `FourCC` newtype in `splica-mp4/src/boxes/mod.rs` with named constants — correct, not magic strings.
- `VideoCodec`/`AudioCodec` enums in `splica-core/src/media.rs` with `Other(String)` escape hatches — intentional.
- Encoder wiring extracted to `splica-cli/src/commands/process/wiring.rs` (Sprint 25 refactor).
- `DemuxerWithConfigs` is a named struct with `demuxer`, `video_tracks`, `audio_tracks` fields (Sprint 20).

## Known Tech Debt

### Active 500-line trigger files (debt sprint is overdue)
- **`splica-core/src/error.rs` — 631 non-test lines** — NEW Sprint 25. Extract serde impls to `error/serde_impls.rs`.
- **`splica-mp4/src/fmp4_muxer.rs` — 540 non-test lines** — carried from Sprint 24. Extract box helpers to `fmp4_box_builders.rs`.

### P1 items (must fix; some past 2-sprint deadline)
- **H.264 flush zero PTS** (`h264/decoder.rs:297`) — silent wrong output on B-frames. 3 sprints deferred. Add `last_emitted_dts` field, increment in flush loop.
- **`open_webm_configs` drops VP9/AV1 video tracks** (`process/args.rs:326`) — 3 sprints deferred. Wire like `open_mkv_configs` using `webm.codec_private()`.
- **Audio encoder bitrate hardcoded 128 kbps** (`wiring.rs:197,208`) — 5 sprints deferred. Add `--audio-bitrate` flag.
- **`splica-webm/src/demuxer/parsing.rs` has no tests** — 455 lines pure parsing, untested. 3 sprints deferred.
- **No subtitle passthrough integration tests** — SPL-152 shipped in Sprint 25 with zero binary-level test coverage.

### P2 items (carry limit watch)
- **`#[allow(dead_code)]` on `Mp4Track` struct** (`track.rs:14`) — 3 sprints; field-level audit needed.
- **`ContainerFormat::is_writable` redundant arm** (`media/mod.rs:160`) — both arms return true.
- **`parse_bitrate` copy-paste for M/m and k/K** (`args.rs:83-111`) — normalize suffix before branching.
- **`cluster_start` dead field suppressed** (`webm/demuxer/mod.rs:34`) — delete or promote.
- **AV1 encoder speed hardcoded to 6** (`wiring.rs:73`) — not connected to `--preset`. 3 sprints deferred.
- **Deprecated `convert`/`transcode` subcommands untested** (`main.rs:185-343`).
- **H.264 flush zero PTS** (also `.unwrap()` on infallible `Timestamp::new(0,1)` — cosmetic but wrong).
- **`detect_ebml_doctype` hand-rolled byte scanner** (`format_detect.rs`) — untested. 4 sprints deferred.

## Sprint Cadence — Tech Debt Process

**3:1 model with trigger override.** Sprint 26 MUST be a debt sprint — trigger active on 2 files, cadence 2 sprints overdue.

- Debt sprint fires: any file crosses 500 non-test lines OR 3 feature sprints elapsed.
- P0 items auto-schedule regardless of cadence.
- Medium items cannot be carried more than 2 sprints without explicit priority call.

## Quality Trends

- Sprint 20 (2026-03-14): Debt sprint. Strong cleanup. Quality high going into Sprint 21.
- Sprint 21 (2026-03-14): Feature sprint 1. Minor changes; muxer.rs/demuxer.rs crossed 500 lines (trigger active).
- Sprints 22-24: Feature sprints. fmp4_muxer.rs crossed 500 lines (trigger not addressed).
- Sprint 25 (2026-03-15): Feature sprint. Subtitle passthrough (SPL-152), --audio-codec flag, post-run summary (SPL-154), --max-fps, --allow-color-conversion. Refactors: wiring.rs extracted, h265/sps.rs extracted, media.rs split to color.rs+frame.rs. error.rs now 631 non-test lines (new trigger). Debt sprint is now 2 sprints overdue. Sprint 26 must be debt.
