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

### ACTIVE 500-line triggers
- None. Both Sprint 28 triggers resolved by Sprint 29 debt sprint.
  - `h265/encoder.rs`: 623 → 424 lines (ffi_helpers.rs extracted, 244 lines)
  - `mp4/muxer.rs`: 560 → 414 lines (box_builders.rs extracted, ~300 lines non-test)

### P0 items
- (cleared Sprint 28) Transcode PTS fix — H265/AV1 encoders now use HashMap keyed by frame_count/poc.
- (cleared Sprint 28) Post-resize muxer metadata — output_dimensions now threaded through PipelineBuilder.
- (cleared Sprint 29) H265 PTS poc/frame_count mismatch — SPL-193 switched to src_out.pts from kvazaar directly.

### P1 items
- (cleared Sprint 28) AV1 encoder flush bare `unwrap()` — replaced with `.expect()`.

### P2 items (carry limit watch)
- **`ContainerFormat::is_writable` redundant arm** (`media/mod.rs:160`) — both arms return true. Carry 4 sprints — EXPLICIT PRIORITY CALL REQUIRED Sprint 30.
- **`parse_bitrate` copy-paste for M/m and k/K** (`args.rs:83-111`) — normalize suffix before branching. Carry 4 sprints — EXPLICIT PRIORITY CALL REQUIRED Sprint 30.
- **`cluster_start` dead field suppressed** (`webm/demuxer/mod.rs:34`) — delete or promote. Carry 4 sprints.
- **Deprecated `convert`/`transcode` subcommands untested** (`main.rs:185-343`). Carry 4 sprints.
- **`detect_ebml_doctype` hand-rolled byte scanner** (`format_detect.rs`) — tests added Sprint 28 (SPL-188); now tested. Watch only.

### Watch list
- `splica-webm/src/demuxer/parsing.rs` — 463 non-test lines (near 500, one feature away from trigger)
- `splica-mp4/src/demuxer.rs` — 439 non-test lines
- `splica-webm/src/demuxer/mod.rs` — 416 non-test lines
- `splica-webm/src/muxer.rs` — 415 non-test lines

## Sprint Cadence — Tech Debt Process

**3:1 model with trigger override.**

- Debt sprint fires: any file crosses 500 non-test lines OR 3 feature sprints elapsed.
- P0 items auto-schedule regardless of cadence.
- Medium items cannot be carried more than 2 sprints without explicit priority call.
- Sprint 29 was the debt sprint. Next mandatory debt sprint: Sprint 33 (or earlier if trigger fires). Nearest trigger risk: `webm/demuxer/parsing.rs` at 463 non-test lines.

## Quality Trends

- Sprint 20 (2026-03-14): Debt sprint. Strong cleanup. Quality high going into Sprint 21.
- Sprint 21 (2026-03-14): Feature sprint 1. Minor changes; muxer.rs/demuxer.rs crossed 500 lines (trigger active).
- Sprints 22-24: Feature sprints. fmp4_muxer.rs crossed 500 lines (trigger not addressed).
- Sprint 25 (2026-03-15): Feature sprint. Subtitle passthrough (SPL-152), --audio-codec flag, post-run summary (SPL-154), --max-fps, --allow-color-conversion. error.rs now 631 non-test lines (new trigger). Debt sprint 2 sprints overdue.
- Sprint 26 (2026-03-15): Debt sprint. Cleared all P1 items and both 500-line triggers. One new low-severity issue introduced (AV1 unwrap). Five container/encoder files now in 416-439 line watch range. Trigger acceptance criterion met. Quality trend: positive.
- Sprint 27 (2026-03-16): Test-only sprint (SPL-182 adversarial fixtures, SPL-183 encode matrix, SPL-184 frame rate passthrough). No production code changed. Two P0 correctness bugs discovered.
- Sprint 28 (2026-03-16): Fix sprint (SPL-185/186/187/188/189). Both P0s resolved. AV1 unwrap cleared. EBML doctype now tested. Two 500-line triggers fired: h265/encoder.rs (623 lines) and muxer.rs (560 lines). New medium-severity concern: H265 poc/frame_count key alignment under B-frames. Quality trend: correctness improved; structural debt re-accumulated from fixes.
- Sprint 29 (2026-03-16): Debt sprint. Both 500-line triggers cleared. H265 PTS poc risk resolved (SPL-193). rescale_timestamp now has 5 unit tests with documented overflow risk (SPL-192). No new triggers. Four P2 items now at 4-sprint carry — explicit priority call required Sprint 30. Quality trend: clean. Next scheduled debt sprint: Sprint 33 (unless trigger fires first). webm/demuxer/parsing.rs at 463 lines is the nearest trigger risk.
