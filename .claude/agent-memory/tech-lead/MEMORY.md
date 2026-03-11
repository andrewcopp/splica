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

## Known Tech Debt

### Carried from Sprint 9 (first seen 2026-03-10, not yet resolved)
- **`ProbeTrack.kind`** (`main.rs:637`) — serializes `TrackKind` as a hand-rolled string match rather than `#[serde(rename_all)]` on the enum. Low severity.

### Identified Sprint 9 end-of-sprint review (2026-03-11)
- **`format_codec` duplication** — identical function exists in both `splica-core/src/wasm_types.rs:57` and `splica-cli/src/main.rs:743`. Medium severity.
- **`TranscodeOutput` type alias** — 5-element anonymous tuple `(u64, u64, u64, u64, Vec<TranscodeAudioInfo>)` at `main.rs:1165`. Medium severity.
- **`build_vp9_codec_string` hardcodes profile** (`splica-webm/src/wasm.rs:156`) — always emits `"vp09.00.10.08"` regardless of track metadata. Medium severity.
- **Volume filter silently skips pass-through audio** (`main.rs:1518-1526`) — `--volume` flag only applies to transcoded audio tracks; pass-through tracks are silently unmodified with no user warning. Medium severity.
- **`WasmVideoDecoderConfig::videoDecoderConfig` returns null for H.265/AV1 MP4** (`splica-mp4/src/wasm.rs:135`) — silent null rather than error. Low/medium severity.
- **`drain_decoder_to_muxer` and `drain_audio_decoder_to_muxer` are near-duplicates** (`splica-pipeline/src/lib.rs:312` and `384`) — parallel video/audio drain functions with identical structure. Medium severity.
- **Pipeline has no tests for audio transcode path** — all pipeline tests use video-only mocks. Medium severity.
- **`PipelineBuilder` type parameter dance** — `with_event_handler` returns a new `PipelineBuilder<G>` and re-fields all members; fragile if new fields are added. Medium severity.
- **`exit_code` module is `#[allow(dead_code)]`** (`main.rs:200`) — constants defined but only two of four are used. Low severity.
- **`DemuxerWithConfigs` type alias** (`main.rs:1177`) — inner tuple `(TrackIndex, Vec<u8>, Option<ColorSpace>)` should be a named struct. Low severity.

## Sprint Cadence — Tech Debt Process

**3:1 model with trigger override** (established 2026-03-11).

- Three feature sprints, then one dedicated debt sprint.
- Debt sprint fires early if any single file crosses 500 lines of non-test code.
- Dana runs an end-of-sprint review at the close of every sprint, updating the debt register above.
- During a debt sprint: every file over 500 lines gets under 500, all tests pass, no behavior changes.
- P0 items auto-schedule into the next sprint regardless of cadence.
- Medium-severity items cannot be carried more than two sprints without an explicit priority call.

**Current trigger status:** Both `splica-cli/src/main.rs` (~1571 lines) and `splica-pipeline/src/lib.rs` (~1099 lines) are past the 500-line threshold. A debt sprint is overdue.

## Quality Trends

- Sprint 9 resolved 4 previously-tracked debt items (classify_error, WebM codec constants, HandlerType enum, AudioMode enum). This is a strong debt-paydown sprint.
- First full end-of-sprint review: 2026-03-11. Codebase health is improving.
- Remaining structural concerns are concentrated in `splica-cli/src/main.rs` (already 1571 lines) and `splica-pipeline/src/lib.rs` (1099 lines including tests).
