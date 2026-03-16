# Product Lead Memory

## Adoption Strategy (2026-03-11) — see adoption-strategy.md for full detail

**Philosophy:** Ship when there's pull, not push. Prove the core ffmpeg-alternative promise first. Adoption-expansion features come after product-market fit is demonstrated by real persona adoption.

**Two parked initiatives (execute at PMF signal, not on a sprint schedule):**
- Agent CLI layer: natural language to splica commands, shows its work, teaches the CLI rather than hiding it. Prerequisite: CLI audit first.
- Reliability comparison: failure-behavior table + real case study, NOT throughput benchmarks. Prerequisite: encode matrix complete + error contract documented publicly.

## Key Decision: CLI/API Strategy (2026-03-10)

**Decision:** Ship one API designed from splica's principles. No ffmpeg compatibility layer. Invest migration cost in a `splica migrate` subcommand that translates ffmpeg commands to splica equivalents with plain-English explanations.

**Rationale:**
- "Easy to migrate from" != "identical to" — a compatibility layer imports ffmpeg's mental model
- Dual APIs never sunset — "legacy" mode becomes permanent maintenance burden
- Jordan wants `splica convert input.mp4 output.webm`, not ffmpeg flags under a new name
- North star: if someone has to ask what a flag does, the API design failed

## Key Decision: ScaleFilter default aspect mode = Fit (2026-03-10)

**Decision:** ScaleFilter's default aspect mode must be `Fit` (letterbox), not `Stretch`. Rationale: Stretch silently distorts content. Elena flagged this; `Fit` is correct-by-default.

## Key Design Pattern: Capability without surface area = zero value (2026-03-10)

Every filter/codec/muxer shipped must be reachable from (a) the CLI and (b) PipelineBuilder before the sprint closes. Observed three times: ScaleFilter (Sprint 4), AudioDecoder/AudioEncoder (Sprint 6), AudioFilter (Sprint 6 — still no impl as of Sprint 8, fixed by SPL-71 Sprint 9).

## Key Pattern: Silent failures require immediate remediation (2026-03-10)

**Rule:** Any command that produces technically invalid output without an error is P0. Silent corruption is worse than an outright failure.

## Key Pattern: Structured output is Priya's unlock

Exit code convention: 0=success, 1=bad input (no retry), 2=internal error (retry), 3=resource exhaustion (retry with different limits). NDJSON progress events on `process --format json` shipped in SPL-70 (Sprint 9, done).

## Key Rule: FFI must always be feature-gated

Any crate that adds FFI must simultaneously add a feature flag that excludes it, and CI must verify the pure-Rust build for wasm32-unknown-unknown. Each codec has its own independent flag (`codec-h264`, `codec-h265`, `codec-aac`, `codec-opus`, `codec-av1` — Sprint 12) — they are not folded into a single `native-codecs` umbrella.

## Codebase State as of Sprint 28 complete / Sprint 29 planning (2026-03-16)

Sprints 1–28 complete. Full codec matrix: H.264 (dec+enc), H.265 (dec+enc via kvazaar), AV1 (dec+enc), AAC (dec+enc), Opus (dec+enc). Containers: MP4 (demux+mux), WebM (demux+mux), MKV (demux+mux). Filters: Scale, Volume, Crop. WASM: WasmMp4Demuxer + WasmWebmDemuxer + WasmMkvDemuxer — full packet/config/seek parity. CLI: process, probe, trim, join, extract-audio, migrate. Exit code contract in --help. Subtitle passthrough. Post-run summary. --audio-codec, --audio-bitrate flags. Per-direction feature flags.

**Sprint 28 delivered (P0 fixes + cleanup):**
- SPL-185: Transcode frame rate P0 fixed — encoders now preserve input PTS, not reconstruct from poc
- SPL-186: Resize metadata P0 fixed — container now writes encoded dimensions, not pre-scale dimensions
- SPL-189: VP9 re-encode gives actionable error instead of generic failure
- SPL-187: AV1 encoder flush unwrap replaced with expect + safety comment
- SPL-188: EBML doctype scanner unit tests added

**Sprint 29 is the LAST feature sprint before the mandatory debt sprint.**

**Top 3 priorities for Sprint 29:**
1. SPL-122 — WASM container detection (serves Marcus + Alex simultaneously, 17-sprint carry)
2. SPL-121 — `--audio-codec` flag (Jordan's audio codec selection gap, 17-sprint carry, implicit behavior anti-pattern)
3. SPL-123 — Exit code contract as versioned public artifact (Priya's retry automation unblocked)

**Still open in backlog:** SPL-122, SPL-121, SPL-123, SPL-169 (probe JSON codec params — Elena, post-debt), SPL-170 (WASM H.264 frame decode — no pull signal yet), SPL-145/146 (verify SPL-146 vs done SPL-152 with Dana), SPL-149 (media.rs split — debt sprint candidate).

## Key Decision: trim --format json = single-shot JSON, not NDJSON (2026-03-11)

`trim --format json` emits one JSON object on success: `{start_pts, end_pts, duration_secs, output}`. No `"type"` field — trim has no progress events, so NDJSON is not needed. On failure, emits `ErrorResult` and exits. Consistent with `probe --format json`. SPL-84.

## Key Decision: SPL-85 WASM seek — expose snapped PTS via seek_position(), not trait change (2026-03-11)

`Mp4Demuxer::seek` returns `()` per the `Seekable` trait. To expose snapped PTS to WASM layer without a breaking trait change, add `fn seek_position(&self) -> Option<Timestamp>` directly on `Mp4Demuxer`. Same pattern for `WebmDemuxer`. Do not change the `Seekable` trait signature.

## Key Decision: WASM seek model = timestamp-based with keyframe snapping (2026-03-11)

seekToTimestamp(pts_ms) returns the actual PTS of the preceding keyframe, then readVideoPacket() resumes from there. Managing byte ranges is the demuxer's responsibility. Returned PTS is always <= requested PTS. Both WasmMp4Demuxer and WasmWebmDemuxer. Underlying seek also exposed on non-WASM demuxer types for pipeline use.

## Key Decision: NDJSON event contract — type discriminator on every line (2026-03-11)

**Decision (SPL-83):** All `process --format json` NDJSON lines carry a `"type"` field:
- Progress lines: `{"type":"progress","packets_read":N,...}`
- Final success line: `{"type":"complete","packets_read":N,...}`
- Final error line: `{"type":"error","error_kind":"...","message":"..."}`

The `"status"` field on `ErrorResult` is removed — redundant with `"type"`. This is a breaking change pre-1.0; document in commit message. `TranscodeResult` struct (previously used for success JSON) becomes unused — delete or comment clearly for SPL-84 repurposing. `probe --format json` error output uses `ErrorResult` and gets `"type"` field automatically.

**SPL-86 integration point:** `DemuxError::UnsupportedCodec { codec }` already carries the codec name in its Display impl. SPL-86 just surfaces it in the `message` field — no structural change to the error event needed.

## Key Decision: H.265 library = libde265-rs (2026-03-11)

`libde265-rs` (0.2.1) chosen for H.265 decode. Safe Rust wrapper around libde265 (LGPL). Same FFI-wrapper pattern as openh264. Feature flag: `codec-h265` (not `native-codecs` — each codec independently gatable). The color mapping functions in `h264/sps.rs` must be extracted to a shared module (`splica-codec/src/color.rs`) rather than duplicated.

## Key Decision: Symphonia as AAC decode (2026-03-10)

Symphonia (pure Rust, Apache 2.0) used for AAC decode — WASM-compatible. Avoids fdk-aac licensing issues.

## Key Decision: wasm-pack --target web as baseline (2026-03-10)

Target `web` (ES module, no bundler required). nodejs and bundler targets are progressive enhancements.

## Key Decision: CLI should expose intent, not mechanism (2026-03-10)

`convert` and `transcode` unified into `process` (SPL-56, done Sprint 8). Stream copy vs re-encode is an implementation detail, not a user concept.

## Key Decision: WASM milestones must include a decode acceptance criterion (2026-03-10)

Future WASM sprint issues must include "can a JS caller get decoded data" as an explicit acceptance criterion.

## Key Pattern: Stringly-typed classification = correctness hazard (2026-03-10)

Exit code / JSON routing must be based on typed error variants, not error message substrings. Fixed by SPL-59 (Sprint 8, done).

## Key Decision: Encoding profile/level flags deferred to Sprint 10+ (2026-03-11)

CLI will expose `--h264-profile` and `--h264-level` as additive flags. Not Sprint 9 — no delivery-spec use cases in active user testing yet. Elena surfaced in Round 9.

## Key Decision: Frame-accurate WebM trim deferred (2026-03-11)

Requires decode-and-re-encode to keyframe boundary. Not in 90% use case for splica's target personas.

## Key Process Decision: 3:1 Sprint Cadence with Trigger Override (2026-03-11)

**Decision:** Three feature sprints, then one dedicated tech debt sprint. Debt sprint fires early if any file crosses 500 lines of non-test code.

**Rules:**
- Dana runs end-of-sprint review every sprint, updates debt register
- Debt sprint acceptance: every 500+ line file gets under 500, all tests pass, no behavior changes, Dana signs off
- P0 items auto-schedule into next sprint regardless of cadence
- Medium-severity items cannot carry more than two sprints without explicit priority call

**Rationale:** End-of-sprint review alone produces documentation, not reduction. 1:1 burns half capacity on internals pre-1.0. 3:1 keeps feature momentum while enforcing a structural ceiling. The 500-line trigger prevents the calendar from being an excuse.

## Key Pattern: Dana's severity ratings can be undercooked (2026-03-11)

Dana rated --volume silent no-op (T1) and VP9 hardcoded codec string (T4) as "medium." Both were reclassified to P0 for Sprint 10. Rule: any output that is silently wrong — regardless of how limited the scenario — is P0, not medium. A missing feature is medium. A feature that accepts user input and produces wrong output without warning is P0.

## Notion Workspace Structure

Hub page: https://www.notion.so/31f1326e510281df9ce1cddebcb5c747

```
splica (hub)
├── Product North Star
├── Roadmap — phase-level only
├── Decisions — database (data source ID: c4d6254c-f484-4605-9dfb-fabbdbb84b96)
├── Personas
├── Feedback Rounds — index; child pages per round
│   ├── Round 1 through Round 8 (see prior entries)
│   ├── Round 9 — Post-Sprint 8 (AAC/Opus Encode, WASM Decode API, process Command, Color Passthrough, Typed Errors)
│   ├── Round 10 — Post-Sprint 9 (WebM WASM Packets, Structured Progress, VolumeFilter, Color Contract, Type-Safety Bundle)
│   ├── Round 11 — Post-Sprint 10 (--volume Fix, VP9 Codec String, videoDecoderConfig Discrimination)
│   ├── Round 12 — Post-Sprint 11 (H.265 Decode + Color Passthrough, NDJSON Error Events, trim --format json, WASM Seeking, Unsupported-Codec Errors) [index entry only; page not yet created]
│   ├── Round 13 — Post-Sprint 19 (AV1 CLI Flag, WasmMkvDemuxer, JSON Contract, mod.rs Split) [page created 2026-03-11]
│   └── Round 14 — Post-Sprint 20 Focus Group (Benchmark Demo Sprint Planning) [page created 2026-03-13]
└── Retrospectives — index; child pages per sprint (Sprint 1–17 + template)
    ├── Sprint Report Template (codifies feature + debt sprint formats)
    ├── Sprint 12 Report — MKV write, QC output, ContainerFormat refactor
    ├── Sprint 13 Report — AV1 decode, MKV demux, AV1 fixture
    ├── Sprint 14 Report — AV1 encode, H.265 encode spike, CropFilter, streaming memory
    ├── Sprint 15 Report — H.265 encode, encoder quality params, pre-flight validation, MKV round-trip tests
    ├── Sprint 16 Report (Debt) — main.rs 1960→267, pipeline/lib.rs 1642→17, H.265 PTS fix, volume fix
    ├── Sprint 17 Report — migrate subcommand, WASM audio, H.264 flush fix, process.rs split
    └── Sprint 21 Report — Benchmark Demos [stub created 2026-03-13, to be completed at sprint close]
```

**Living state** (update in place): North Star, Roadmap, Decisions DB, hub "Current focus" callout.
**Historical record** (new pages only, never edit old): Feedback Rounds, Retrospectives.

Three [ARCHIVED] pages exist from old structure — do not use.

## Linear Workspace Conventions

**Team:** Splica (SPL) | **Project:** splica v0.1

**Milestones:**
- Phase 0 through Sprint 28 — all complete
- Sprint 29 — planning (last feature sprint before debt)

**Labels** (domain-based only): `core-infra`, `codec`, `container`, `dx`

**Issue template:** https://linear.app/splica/document/issue-template-73f83bc8aac3

**Conventions:** Always assign to splica v0.1 project + current sprint milestone + at least one domain label. Use blocks/blocked-by for dependencies. No estimates, no target dates yet.

**Deferred:** Cycles, sub-issues, estimates, Views/Initiatives/Triage/SLAs.

## Triage Workflow

1. Ask engineer to investigate codebase — don't guess at code readiness.
2. Record findings in Linear as "Sam's triage note" comment: what was reviewed, ready-to-start status, implementation guidance, risks.
3. Move issues to Todo when confirmed ready.
4. Update Notion if findings affect broader product picture.

## Key Process Decision: Sprint reports are product-thesis instruments, not delivery logs (2026-03-11)

Reports stopped after Sprint 11; backfilled for Sprints 12–14 on 2026-03-11. Format codified in Notion template. Two variants:
- **Feature sprint:** Product Thesis Check (Moved/No movement/Blocked per persona) + What Shipped + What Didn't Ship + Single Biggest Gap
- **Debt sprint:** What Was Resolved + Structural Health (line counts before/after) + What This Unlocks + Single Biggest Remaining Risk

Rule: report written at sprint close, before next sprint planning. Never edited after the fact. "Moved" means a root need got materially closer to satisfied — not that a feature shipped that a persona might use someday.

## Key Decision: H.265 encode library = kvazaar, not x265 (2026-03-11)

**x265 is GPL-2.0** — incompatible with splica's Apache-2.0 license. SPL-87's original description said "LGPL — same as libde265" — this was wrong. kvazaar (University of Tampere) is BSD-3-Clause, fully compatible. Competitive quality at fast-to-medium presets. No existing Rust bindings — requires creating `kvazaar-sys` (bindgen) as a prerequisite. Feature flag: `codec-h265-enc` (separate from `codec-h265` which gates libde265-rs decode — they are different C libraries). VUI color support confirmed in kvazaar API.

## Focus Group Rounds — see focus-group-rounds.md for Rounds 15–18 detail

Round 18 (Post-Sprint 28, 2026-03-16) key findings:
- Both P0s fixed: PTS/frame rate (H.265 + AV1 transcode) and resize metadata
- Sprint 29 top 3: SPL-122 (WASM container detection), SPL-121 (--audio-codec flag), SPL-123 (exit code contract versioned)
- SPL-169 (probe JSON codec params) deferred post-debt — additive, no urgency
- SPL-170 (WASM H.264 frame decode) still deferred — no pull signal from real JS callers
- Sprint 29 is the LAST feature sprint before mandatory debt sprint

## Sprint 21: Benchmark Demos — COMPLETE (2026-03-14)

All 8/8 issues shipped. Honest wins demonstrated: CLI ergonomics (3 tokens vs 7+), structured errors (typed exit codes), correct-by-default AR (letterbox), browser WASM (no ffmpeg equivalent), documented memory model. Raw encode throughput intentionally not benchmarked — splica will not win that cleanly. Round 14 focus group posted. Sprint 21 retro stub in Notion (to complete at sprint close).

## Key Triage Finding: MKV EBML reuse approach (2026-03-11)

Do NOT move EBML primitives from splica-webm to splica-core. splica-core is types/traits only. For MKV, either duplicate the minimal EBML write primitives (they are small) or expose them as pub(crate) from splica-webm via workspace-internal re-export. The only structural difference between WebM and MKV EBML is the DocType header (`webm` vs `matroska`) and codec IDs. MKV codec IDs: H.264=`V_MPEG4/ISO/AVC`, H.265=`V_MPEGH/ISO/HEVC`, AAC=`A_AAC`, Opus=`A_OPUS`. Getting these wrong produces well-formed files that reference players reject silently — Elena will notice.

