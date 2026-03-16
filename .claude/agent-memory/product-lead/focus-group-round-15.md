---
name: Focus Group Round 15 — Post-Sprint 25 Findings
description: Key product insights and P0 issues discovered in the Round 15 focus group (2026-03-15)
type: project
---

Focus group conducted 2026-03-15 against Sprint 25 codebase. Full report in conversation history.

**Why:** Identifying gaps before Sprint 26 planning.
**How to apply:** Use these findings to prioritize Sprint 26 scope.

## P0 Issues (silent wrong output — must schedule into Sprint 26)

**1. Frame rate silent downsampling (Jordan + Elena — blocking)**
The `reencode` path hardcodes frame rate hint: fast=30fps, medium=30fps, slow=60fps. A 60fps source processed with medium preset gets 30fps output with no warning. Post-run summary does not show the applied frame rate. Fix: default to source frame rate unless `--max-fps` is explicitly set. The current behavior is an implicit-output anti-pattern.

**2. Silent bitrate default during re-encode (Elena — blocking)**
When neither `--bitrate` nor `--crf` is given, preset falls back to 500k/1M/2M. This is content-blind and produces delivery-failing output for high-res content. Fix: default to CRF-based encoding when no quality target is specified (content-adaptive, no guesswork). Or: prominently warn in post-run summary that a default was applied.

## High-Priority Issues (painful, schedule within 2 sprints)

**3. No liveness signal from `trim` and `join` (Priya — blocking for health-check SLAs)**
`trim` is stream-copy and emits a single JSON object at completion. `join` same. For long-running operations, Priya's K8s health checks time out. `process` already has NDJSON progress — the pattern exists. `join` result also missing `duration_seconds` field (requires second `probe` call to validate output duration).

**4. WASM encode path absent (Alex — blocking for browser editing use case)**
All three containers have WASM demuxers. None have WASM encoders or muxers. rav1e (AV1 encoder) is pure Rust and already in the dependency tree — it compiles to wasm32-unknown-unknown. Missing: `WasmAv1Encoder` binding and `WasmMp4Muxer`/`WasmWebmMuxer` that finalize to `Uint8Array`. This is the entire browser-side transcode use case.

## Medium Issues (one sprint carry max)

**5. Custom I/O path for library consumers is undocumented (Marcus — painful)**
`PipelineBuilder::with_demuxer` accepts any `impl Demuxer + 'static`, so `Cursor<Vec<u8>>`-backed demuxers work. But there is no public example showing this. The `TestDemuxer` in the traits test suite proves the pattern but is private. One public doc example on `PipelineBuilder` removes the friction with zero code changes.

## Validated product decisions (do not regress)

- Exit code contract (0/1/2/3) with typed `error_kind` — serves both Priya and Jordan
- `--allow-color-conversion` as a gate (error, not warning) — Elena's correctness anchor
- `validate()` separate from `build()` — serves Priya's pre-flight orchestration need
- Aspect mode default = Fit — correct by default, no flag required

## Backlog items confirmed still open

- EBU R128 loudness normalization (Elena, post-v0.1)
- Batch processing / directory mode (Jordan, post-v0.1)
- ResourceBudget memory cap on PipelineBuilder (Marcus, post-v0.1)
