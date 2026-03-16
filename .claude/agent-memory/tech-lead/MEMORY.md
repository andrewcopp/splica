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

### ACTIVE 500-line triggers (Sprint 28)
- `splica-codec/src/h265/encoder.rs` — 623 non-test lines. Trigger ACTIVE. PTS fix added HashMap + fallback logic.
- `splica-mp4/src/muxer.rs` — 560 total, no test block. Trigger ACTIVE. rescale_timestamp helper added Sprint 28.

### P0 items
- (cleared Sprint 28) Transcode PTS fix — H265/AV1 encoders now use HashMap keyed by frame_count/poc.
- (cleared Sprint 28) Post-resize muxer metadata — output_dimensions now threaded through PipelineBuilder.
- **H265 PTS HashMap key mismatch risk** — encoder inserts `frame_count as i32` but reads back `info_out.poc`. kvazaar's poc in a B-frame stream may not equal frame_count. Needs verification. Root: `h265/encoder.rs:485-486`. Medium effort. Discovered Sprint 28 review.

### P1 items
- (cleared Sprint 28) AV1 encoder flush bare `unwrap()` — replaced with `.expect()`.

### P2 items (carry limit watch)
- **`ContainerFormat::is_writable` redundant arm** (`media/mod.rs:160`) — both arms return true. Carry 3+ sprints — MUST call explicit priority next sprint.
- **`parse_bitrate` copy-paste for M/m and k/K** (`args.rs:83-111`) — normalize suffix before branching. Carry 3+ sprints — MUST call explicit priority next sprint.
- **`cluster_start` dead field suppressed** (`webm/demuxer/mod.rs:34`) — delete or promote. Carry 3+ sprints.
- **Deprecated `convert`/`transcode` subcommands untested** (`main.rs:185-343`). Carry 3+ sprints.
- **`detect_ebml_doctype` hand-rolled byte scanner** (`format_detect.rs`) — tests added Sprint 28 (SPL-188); now tested. Downgrade from ESCALATE to watch.

### Watch list
- `splica-webm/src/demuxer/parsing.rs` — 464 non-test lines (approaching 500)
- `splica-mp4/src/demuxer.rs` — ~439 non-test lines
- `splica-webm/src/demuxer/mod.rs` — ~417 non-test lines
- `splica-webm/src/muxer.rs` — ~416 non-test lines

## Sprint Cadence — Tech Debt Process

**3:1 model with trigger override.**

- Debt sprint fires: any file crosses 500 non-test lines OR 3 feature sprints elapsed.
- P0 items auto-schedule regardless of cadence.
- Medium items cannot be carried more than 2 sprints without explicit priority call.
- Sprint 28 is feature sprint 2 of the current cycle. Next mandatory debt sprint: Sprint 30 (or Sprint 29 if trigger fires — it has fired on h265/encoder.rs and muxer.rs).

## Quality Trends

- Sprint 20 (2026-03-14): Debt sprint. Strong cleanup. Quality high going into Sprint 21.
- Sprint 21 (2026-03-14): Feature sprint 1. Minor changes; muxer.rs/demuxer.rs crossed 500 lines (trigger active).
- Sprints 22-24: Feature sprints. fmp4_muxer.rs crossed 500 lines (trigger not addressed).
- Sprint 25 (2026-03-15): Feature sprint. Subtitle passthrough (SPL-152), --audio-codec flag, post-run summary (SPL-154), --max-fps, --allow-color-conversion. error.rs now 631 non-test lines (new trigger). Debt sprint 2 sprints overdue.
- Sprint 26 (2026-03-15): Debt sprint. Cleared all P1 items and both 500-line triggers. One new low-severity issue introduced (AV1 unwrap). Five container/encoder files now in 416-439 line watch range. Trigger acceptance criterion met. Quality trend: positive.
- Sprint 27 (2026-03-16): Test-only sprint (SPL-182 adversarial fixtures, SPL-183 encode matrix, SPL-184 frame rate passthrough). No production code changed. Two P0 correctness bugs discovered.
- Sprint 28 (2026-03-16): Fix sprint (SPL-185/186/187/188/189). Both P0s resolved. AV1 unwrap cleared. EBML doctype now tested. Two 500-line triggers fired: h265/encoder.rs (623 lines) and muxer.rs (560 lines). New medium-severity concern: H265 poc/frame_count key alignment under B-frames. Quality trend: correctness improved; structural debt re-accumulated from fixes.
