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

### No active 500-line trigger files (Sprint 26 debt sprint cleared both; Sprint 27 no production code changed)

### P0 items (auto-schedule — silent wrong output)
- **Transcode frame rate broken** — encoders use `poc` as PTS ticks in encoder timebase, but encoders receive frames with input-native ticks. H.265→H.264 transcode produces 2.67fps; AV1→H.264 produces 98fps. Root: `h265/encoder.rs:274-281`, `av1/encoder.rs` analogous. Medium effort. Discovered SPL-184.
- **Post-transcode muxer has wrong resolution metadata** — `Mp4Muxer::add_track` stores original `TrackInfo` dimensions; after resize re-encode the muxed tkhd/stsd report the pre-scale resolution. Root: `mp4/src/muxer.rs:367-408` / `reencode.rs` wire path. Medium effort. Discovered encode matrix test.

### P1 items (open)
- **AV1 encoder flush: bare `unwrap()`** (`av1/encoder.rs:335,351`) — Sprint 26. Replace with `.expect()` + comment. Small effort. Carry sprint 2/2 — must schedule next sprint.

### P2 items (carry limit watch)
- **`ContainerFormat::is_writable` redundant arm** (`media/mod.rs:160`) — both arms return true. Carry 2+ sprints.
- **`parse_bitrate` copy-paste for M/m and k/K** (`args.rs:83-111`) — normalize suffix before branching. Carry 2+ sprints.
- **`cluster_start` dead field suppressed** (`webm/demuxer/mod.rs:34`) — delete or promote. Carry 2+ sprints.
- **Deprecated `convert`/`transcode` subcommands untested** (`main.rs:185-343`). Carry 2+ sprints.
- **`detect_ebml_doctype` hand-rolled byte scanner** (`format_detect.rs`) — untested. 6 sprints deferred. ESCALATE.

### Watch list (approaching 500 lines — no change Sprint 27, no production code modified)
- `splica-mp4/src/demuxer.rs` — ~440 non-test lines
- `splica-codec/src/h265/encoder.rs` — ~438 non-test lines
- `splica-webm/src/demuxer/mod.rs` — ~418 non-test lines
- `splica-mp4/src/muxer.rs` — ~418 non-test lines
- `splica-webm/src/muxer.rs` — ~417 non-test lines

## Sprint Cadence — Tech Debt Process

**3:1 model with trigger override.**

- Debt sprint fires: any file crosses 500 non-test lines OR 3 feature sprints elapsed.
- P0 items auto-schedule regardless of cadence.
- Medium items cannot be carried more than 2 sprints without explicit priority call.
- Sprint 27 is feature sprint 1 of the new cycle. Next mandatory debt sprint: Sprint 30 (or earlier if trigger fires).

## Quality Trends

- Sprint 20 (2026-03-14): Debt sprint. Strong cleanup. Quality high going into Sprint 21.
- Sprint 21 (2026-03-14): Feature sprint 1. Minor changes; muxer.rs/demuxer.rs crossed 500 lines (trigger active).
- Sprints 22-24: Feature sprints. fmp4_muxer.rs crossed 500 lines (trigger not addressed).
- Sprint 25 (2026-03-15): Feature sprint. Subtitle passthrough (SPL-152), --audio-codec flag, post-run summary (SPL-154), --max-fps, --allow-color-conversion. error.rs now 631 non-test lines (new trigger). Debt sprint 2 sprints overdue.
- Sprint 26 (2026-03-15): Debt sprint. Cleared all P1 items and both 500-line triggers. One new low-severity issue introduced (AV1 unwrap). Five container/encoder files now in 416-439 line watch range. Trigger acceptance criterion met. Quality trend: positive.
- Sprint 27 (2026-03-16): Test-only sprint (SPL-182 adversarial fixtures, SPL-183 encode matrix, SPL-184 frame rate passthrough). No production code changed. No trigger fires. Two P0 correctness bugs discovered: transcode frame rate is completely wrong (PTS reconstruction broken in H265/AV1 encoders), and post-resize muxer metadata reports wrong resolution. AV1 flush unwrap now at carry limit (sprint 2/2). `detect_ebml_doctype` at 6 sprints deferred — escalation warranted. Quality trend: stable structurally; correctness surface area increased with test coverage.
