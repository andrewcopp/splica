# Tech Lead Memory

## Codebase Overview

splica is a Rust-based media processing library and CLI. Workspace crates:
`splica-core` → `splica-mp4`, `splica-webm`, `splica-mkv` → `splica-codec` → `splica-filter` → `splica-pipeline` → `splica-cli`.

Key architectural decisions confirmed:
- `FourCC` newtype in `splica-mp4/src/boxes/mod.rs` with named constants (FTYP, MOOV, etc.) — correct pattern, not a magic-string problem.
- Codec identity represented as `VideoCodec`/`AudioCodec` enums in `splica-core/src/media.rs` with `Other(String)` escape hatches — intentional design, documented.
- `HandlerType` enum (`splica-mp4/src/boxes/hdlr.rs`) — RESOLVED in Sprint 9. Now a proper enum with Video/Audio/Other variants.
- WebM codec ID strings — RESOLVED in Sprint 9. Now constants in `splica-webm/src/elements.rs`.
- `classify_error` in CLI — RESOLVED in Sprint 9. Now uses typed `ErrorKind` downcasting.
- `TranscodeAudioInfo.mode` field — RESOLVED in Sprint 9. Now `AudioMode` enum.
- `WasmVideoDecoderConfig` for H.265/AV1 — RESOLVED in Sprint 16. MP4 WASM now returns config for all three codecs.
- Volume filter silent skip — RESOLVED in Sprint 16. `volume_requested` flag forces audio transcode.
- `H265Decoder` flush PTS (SPL-102) — RESOLVED in Sprint 16. PTS now from `image.get_image_pts()`.
- `H264Decoder::flush_remaining` multi-frame bug — RESOLVED in Sprint 17 (SPL-106). Now uses VecDeque, flushes all frames.
- `splica-cli/src/commands/process.rs` 972-line threshold violation — RESOLVED in Sprint 17 (SPL-105). Split into process/mod.rs, args.rs, stream_copy.rs, reencode.rs.
- `DemuxerWithConfigs` inner 6-tuple — PARTIALLY RESOLVED Sprint 17. Per-track tuple replaced by named `VideoTrackConfig`. The top-level `DemuxerWithConfigs` type alias remains a 3-tuple (demuxer + 2 vecs), not a named struct. Minor residual.

## Known Tech Debt

### Resolved items
- `build_vp9_codec_string` hardcodes profile — RESOLVED Sprint 17.
- `PipelineBuilder` type parameter dance — RESOLVED Sprint 18 (SPL-112).
- `TrackMode::Copy` dead code — RESOLVED Sprint 18 (removed with PipelineBuilder refactor).
- `extract_audio` command has no JSON output — RESOLVED Sprint 18 (SPL-114).

### Resolved in Sprint 20 (debt sprint)
- **`parse_resize` called twice per track** — RESOLVED Sprint 20. `enc_w`/`enc_h` now computed once before encoder branch.
- **`DemuxerWithConfigs` type alias** — RESOLVED Sprint 20. Now a named struct with `demuxer`, `video_tracks`, `audio_tracks` fields.
- **`open_mkv_configs` returns empty video track configs** — RESOLVED Sprint 20. MKV video tracks now wired via `codec_private()`.
- **EBML element constants duplication** — RESOLVED Sprint 20 (commit 9caa588). `splica-mkv/src/elements.rs` now re-exports from `splica-webm`.
- **`exit_code::SUCCESS` dead code** — RESOLVED Sprint 20 (commit 8b3660a).

### Carried from Sprint 16 end-of-sprint review (2026-03-11)
- **`splica-core/src/media.rs` is 984 lines** (~650 non-test) — needs splitting. Medium severity. 4 sprints carried.
- **`splica-codec/src/h264/encoder.rs` is 614 lines** (~406 non-test) — approaching threshold. Low/medium severity. 4 sprints carried.

### Identified Sprint 17 end-of-sprint review (2026-03-11)
- **`migrate` silently drops flags on trim path** — RESOLVED in Sprint 18 (SPL-111). `collect_trim_warnings` now emits warnings into the explanation list for all dropped flags.
- **`H264DecoderConfig::max_ref_frames` always returns 0** (`h264/decoder.rs:152`) — stale comment, no issue tracked. Low severity. 2 sprints carried.
- **Audio encoder bitrate hardcoded to 128 kbps** (`reencode.rs:391,402`) — no `--audio-bitrate` CLI flag. Low/medium severity. 2 sprints carried.
- **`WasmAudioPacket::duration_us` always -1.0** (`mp4/wasm.rs:272`, `webm/wasm.rs:237`) — both `read_audio_packet` implementations pass `-1.0`. Medium severity. 2 sprints carried.
- **No CLI integration tests for `migrate`** — unit tests exist but no fixture-level binary test. Low/medium severity. 2 sprints carried.

### Identified Sprint 18 end-of-sprint review (2026-03-11)
- **`detect_ebml_doctype` is a hand-rolled EBML parser** (`format_detect.rs`) — MOVED to `format_detect.rs` in Sprint 19 refactor. Now a standalone 70-line byte scanner. Still fragile (scans for 2-byte DocType ID without using splica-webm's proper EBML parser), still untested in isolation. Medium severity. 2 sprints carried.
- **`splica-mkv/src/ebml.rs` and `splica-mkv/src/elements.rs` duplicate `splica-webm`** — Both element tables define the same constants with identical values. `splica-webm` has a proper EBML parser; `splica-mkv` has write-only EBML helpers. Medium severity (maintenance burden, drift risk). 2 sprints carried.
- **`splica-mkv/src/muxer.rs` is 383 lines** — entirely production code, approaching the 500-line trigger threshold. No tests in the file. Low severity (watch). 2 sprints carried.
- **`splica-cli/src/commands/mod.rs` `SUCCESS` exit code has `#[allow(dead_code)]`** (`mod.rs:71`) — constant defined but never referenced. Low severity. 2 sprints carried.

### Identified Sprint 19 (still open at Sprint 21 review)
- **`WasmAudioPacket::duration_us` always -1.0** (SPL-120) — `mp4/wasm.rs`, `webm/wasm.rs`, `mkv/wasm.rs` — all three fixed in Sprint 20 via `compute_audio_frame_duration`. RESOLVED Sprint 20.
- **`--codec av1` for non-WebM outputs** — integration test added in Sprint 20 (commit 4ce7212). RESOLVED Sprint 20.
- **`splica-webm/src/demuxer.rs` is 849 lines** — SPLIT in Sprint 20 into `demuxer/mod.rs` (412 lines) + `demuxer/parsing.rs` (455 lines). Trigger no longer active; `parsing.rs` has no tests though.
- **Audio encoder bitrate hardcoded to 128 kbps** (`reencode.rs:416,427`) — no `--audio-bitrate` CLI flag. Low/medium severity. NOW 4 sprints carried.
- **No CLI integration tests for `migrate`** — binary invocation test added (4ce7212). RESOLVED Sprint 20.
- **`H264DecoderConfig::max_ref_frames` always returns 0** (`h264/decoder.rs:152`) — stale comment. Low severity. NOW 4 sprints carried.
- **`detect_ebml_doctype` is a hand-rolled byte scanner** (`format_detect.rs`) — still untested in isolation. Medium severity. NOW 3 sprints carried.

### Identified Sprint 21 end-of-sprint review (2026-03-14)
- **`build_h264_codec_string` / `build_vp9_codec_string` / `compute_audio_frame_duration` duplicated 3×** across `mp4/wasm.rs`, `webm/wasm.rs`, `mkv/wasm.rs` — logic-identical functions with different concrete receiver types. Move to `splica-core/src/wasm_types.rs`. Medium severity.
- **H.264 decoder flush uses hardcoded zero PTS** (`h264/decoder.rs:297`) — `Timestamp::new(0, 1)` for all flushed frames. Low severity.
- **`splica-mp4/src/muxer.rs` is 552 lines** (all production, no test block) — 500-line trigger is ACTIVE for production code. High priority for debt sprint.
- **`splica-mp4/src/demuxer.rs` is 527 lines** (all production, no test block) — above 500-line threshold.
- **`open_webm_configs` silently drops video tracks** (`process/args.rs:322`) — comment says "WebM doesn't expose MP4-style codec config". This means `splica process input.webm output.webm` will error. The WebM demuxer does expose `codec_private()` for VP9/AV1. Medium severity (user-facing error path without clear message).
- **`ContainerFormat::is_writable` has a redundant match arm** (`media.rs:155-159`) — both arms return `true`; the second `Self::Mkv => true` arm is unreachable because the first arm covers all variants. Low severity.
- **`#[allow(dead_code)]` on `Mp4Track` struct** (`track.rs:14`) — struct-level suppression hides which specific fields (e.g., `track_id`) are unused. Low severity.
- **`read_next_cluster` is mutually recursive** (`demuxer/mod.rs:197`) — calls itself to skip non-cluster elements; unbounded recursion on pathologically interleaved files. Low/medium severity.
- **`parse_bitrate` / `parse_crop` / `parse_volume` have no unit tests** (`process/args.rs`) — these parsing helpers are public-ish and parse user input, but have no test block. Medium severity.
- **`splica-cli` test helpers duplicated across 4 test files** — `splica_binary()`, `fixture_path()`, `test_dir()` each defined independently in `fixture_tests.rs`, `mkv_tests.rs`, `malformed_input_tests.rs`, `trim_test.rs`. Low severity.
- **AV1 encoder speed hardcoded to 6** (`reencode.rs:296`) — not connected to the `--preset` flag. Low severity.
- **`splica-webm/src/demuxer/parsing.rs` has no test block** (455 lines of pure parsing logic) — pure functions with no tests. Medium severity.

## Sprint Cadence — Tech Debt Process

**3:1 model with trigger override** (established 2026-03-11).

- Three feature sprints, then one dedicated debt sprint.
- Debt sprint fires early if any single file crosses 500 lines of non-test code.
- Dana runs an end-of-sprint review at the close of every sprint, updating the debt register above.
- During a debt sprint: every file over 500 lines gets under 500, all tests pass, no behavior changes.
- P0 items auto-schedule into the next sprint regardless of cadence.
- Medium-severity items cannot be carried more than two sprints without an explicit priority call.

**Current trigger status (Sprint 21 end):** `splica-mp4/src/muxer.rs` is 552 lines of production-only code — the 500-line trigger IS ACTIVE. `splica-mp4/src/demuxer.rs` is 527 lines (also production-only). Sprint 21 was a feature sprint; Sprint 22 should be a debt sprint (Sprint 20 was the debt sprint; 21 completes one feature sprint of the new 3:1 cycle). Recommend treating Sprint 22 as debt-focused given the active trigger. `splica-core/src/media.rs` is 984 lines (~649 non-test); split still pending 4+ sprints. `splica-codec/src/h264/encoder.rs` is 614 lines (~405 non-test). `splica-webm/src/demuxer/parsing.rs` is 455 lines with no tests.

## Quality Trends

- Sprint 9 (2026-03-11): strong debt-paydown, resolved 4 tracked items.
- Sprint 15 (2026-03-11): H.265 encoder, MKV support, pipeline validation shipped with decent test coverage. No previously-tracked debt resolved.
- Sprint 16 (2026-03-11): DEBT SPRINT — resolved the two trigger violations (main.rs 1960→267, pipeline/lib.rs 1642→17). Fixed 3 correctness bugs (H.265 PTS, H.265 decode loop, volume skip). Added H.265 integration test. However, process.rs is now itself 972 lines — the extraction created a new threshold violation.
- Sprint 17 (2026-03-11): Feature sprint. Resolved process.rs trigger violation via module split (SPL-105). Fixed H.264 multi-frame flush (SPL-106). Added AAC and Opus codec integration tests (SPL-107). Added WASM audio packet API (SPL-108). Added `migrate` subcommand (SPL-109). VP9 codec string now properly parses CodecPrivate. New debt: migrate flag-dropping bug on trim path (high), migrate.rs approaching size threshold, audio bitrate always hardcoded, WasmAudioPacket duration always -1.0.
- Sprint 18 (2026-03-11): Feature sprint (3rd of cycle). MKV demuxer added (SPL-110). PipelineBuilder type parameter eliminated (SPL-112). migrate now warns on dropped trim flags (SPL-111). Probe gets container-level metadata (SPL-113). extract-audio gets --format json (SPL-114). Resolved: extract_audio JSON output (carried from Sprint 16), migrate flag-dropping bug. New debt: mod.rs at 444 lines (growing), duplicated EBML code between splica-mkv and splica-webm, no tests for probe container metadata fields or extract-audio JSON, detect_ebml_doctype is a hand-rolled parser when splica-webm/src/ebml already exists. TrackMode::Copy dead code removed (confirmed — variant is gone after PipelineBuilder refactor).
- Sprint 19 (2026-03-11): Feature sprint (completes 3:1 cycle). WasmMkvDemuxer added. Per-track duration populated in WebM/MKV demuxers. --codec av1 flag added for non-WebM outputs. Trim JSON type discriminator added.
- Sprint 20 (2026-03-14): Debt sprint. Resolved: parse_resize double-call, DemuxerWithConfigs named struct, open_mkv_configs video wiring, EBML constants consolidation (re-export from splica-webm), exit_code::SUCCESS dead code, WasmAudioPacket duration fix, --codec av1 integration test, migrate binary test. Split splica-webm/src/demuxer.rs into demuxer/mod.rs + demuxer/parsing.rs. Quality strong going into Sprint 21.
- Sprint 21 (2026-03-14): Feature sprint (1st of new 3:1 cycle). Sprint 21 changes appear minor from git log (CI fixes, exit code constant cleanup). Key threshold: splica-mp4/src/muxer.rs and demuxer.rs now cross 500 lines of production code — the 500-line trigger is active. New debt identified: WASM codec string helpers duplicated across 3 files, parse_bitrate/parse_crop no tests, read_next_cluster recursion, parsing.rs has no tests, WebM video re-encode gap. Audio bitrate hardcoded and H264 flush zero-PTS continue to be deferred (now 4 sprints). Sprint 22 should be a debt sprint given active 500-line trigger.
