---
name: Focus Group Rounds 15–18
description: Detailed persona findings and sprint priority calls from post-sprint focus groups
type: project
---

## Round 15 — Post-Sprint 25 (2026-03-15)

P0: (1) Preset re-encode silently downsamples frame rate to 30fps for fast/medium presets — fix by defaulting to source frame rate unless --max-fps set. (2) Preset re-encode uses content-blind bitrate default — fix by defaulting to CRF.
High: (3) trim + join have no liveness signal; join result missing duration_seconds. (4) WASM encode path absent — rav1e is pure Rust, prerequisite met.
Medium: (5) Custom I/O path for PipelineBuilder undocumented.

## Round 16 — Post-Sprint 26 (2026-03-15)

Debt sprint, persona movement secondary.
- Jordan: VP9/AV1 WebM fix (SPL-174) was biggest win. Gap: process summary shows codec mode but not effective settings (bitrate used). Minor.
- Priya: H.264 flush PTS fix (SPL-173) removes silent corruption vector. Gap: SPL-123 (exit code contract not versioned/linkable). Painful.
- Marcus: SPL-169 (probe JSON profile/level/color) still Low, blocks conditional transcoding. Should be reclassified Medium.
- Elena: SPL-126 frame rate passthrough tests must cover encode path (not stream copy — that's already correct by construction). Painful.
- Alex: SPL-122 (unified WASM container detection) highest-leverage single WASM change. In backlog since Sprint 11.

Top Sprint 27 priorities: (1) SPL-180 adversarial fixtures, (2) SPL-181 encode matrix + SPL-126 transcode path, (3) SPL-122 WASM unified entry, (4) SPL-169 probe JSON (reclassify Medium), (5) confirm SPL-146 duplicate of done SPL-152.

## Round 17 — Post-Sprint 27 (2026-03-16)

Sprint 27 delivered: adversarial fixtures (11 tests), encode matrix (23 tests), frame rate passthrough tests. Tests found a P0 before it reached users.

**P0 discovered:** H.265 transcode 30fps→2.67fps. AV1 transcode 30fps→98fps, duration 10s→111s. Both silent (exit 0). Root cause in pipeline transcode decode-encode roundtrip.

**Red cells from encode matrix:**
- Probe reports pre-scale resolution after ScaleFilter — QC correctness failure
- VP9 decode blocks re-encode — needs explicit error, not silent failure (ffmpeg anti-pattern)
- WebM/AV1 output blocked by rav1e speed in debug — CI reliability gap, needs #[ignore] with explanation

**Persona reactions to P0:**
- Jordan: no signal the output was wrong — trust damage disproportionate to bug severity
- Priya: exits 0, automation moves on, downstream receives corrupt files — retry logic is helpless
- Marcus: pipeline API returning malformed VideoFrame structs — abstraction is negative-cost
- Elena: transcode is the core operation — would distrust all output until regression suite proves timestamp integrity
- Alex: WebCodecs API may reject 98fps stream; rav1e/debug gap makes that matrix cell unverifiable in CI

**Sprint 28 recommended scope:**
- P0 (mandatory): Fix transcode timestamp/frame rate bug — both H.265 and AV1 paths — with regression test in encode matrix before closing
- High: Fix probe-after-scale resolution reporting (Jordan + Elena QC workflows)
- High: SPL-123 — exit code contract as versioned public artifact (Priya's root need)
- Medium: VP9 re-encode → explicit unsupported error (not silent failure)
- Medium: WebM/AV1/rav1e-debug → mark matrix cells #[ignore] with explanation
- Deferred to Sprint 29: SPL-122 (WASM container detection), SPL-170 (WASM H.264 frame decode) — don't mix correctness triage with feature work

**Sprint 28 thesis:** Turn Sprint 27's red cells green. A clean matrix is the prerequisite for the credibility claim that splica handles the 90% case correctly — and the prerequisite for everything after (reliability comparison, adoption initiatives, benchmark demos).

## Round 18 — Post-Sprint 28 (2026-03-16)

Sprint 28 delivered: both P0s fixed (PTS/frame rate + resize metadata), VP9 clear error, AV1 unwrap cleanup, EBML doctype tests. Sprint 28 is the 2nd of 3 feature sprints before mandatory debt sprint.

**Persona reactions:**
- Jordan: post-run summary + correct resize metadata = trusts output again. Top request: SPL-121 (`--audio-codec` flag) — still doesn't know what audio codec she's getting without probing.
- Priya: VP9 explicit error is the signal she needed for pipeline routing. Top request: SPL-123 — exit code contract not linkable/versioned, retry logic betting on invisible behavior.
- Marcus: Frame rate correctness mattered more than he flagged (wrong PTS = downstream decoder stutter). Top request: SPL-122 — three parallel JS code paths is not zero-cost abstraction.
- Elena: Two P0s fixed in one sprint = trust signal. Watching encode matrix red cells closely. Top request: SPL-169 (probe JSON codec params — profile, level, color primaries for machine-readable QC).
- Alex: SPL-122 is his top request too — can't build an SDK on three parallel container imports. 17-sprint carry.

**Sprint 29 priorities (last feature sprint before debt):**
1. SPL-122 — WASM container detection (Marcus + Alex, 17-sprint carry, unblocks SDK story)
2. SPL-121 — `--audio-codec` flag (Jordan, 17-sprint carry, implicit audio selection is ffmpeg anti-pattern)
3. SPL-123 — Exit code contract as versioned artifact (Priya, automation correctness)

**Deferred:** SPL-169 (Elena, additive, no urgency — post-debt). SPL-170 (Alex, no pull signal from real JS callers).

**Note on MEMORY.md line count:** MEMORY.md is at/near 200-line truncation limit. Trim before Sprint 30 planning — move older decisions to topic files.
