---
name: Stress-Test Personas (Broader Adoption)
description: Five harder-to-please personas designed to expose gaps blocking broader adoption beyond the original five friendly personas
type: project
---

Five stress-test personas designed 2026-03-16. These represent skeptical, critical users — not friendly early adopters.

**Why:** Founder asked for personas that stress-test readiness for broader adoption. Original five (Jordan/Priya/Marcus/Elena/Alex) are satisfied. These expose what would need to be true for splica to win harder markets.

## The Five Personas

**Ravi (High-Volume Media Operations)**
- Streaming platform running 50k files/day
- Root need: predictable per-worker throughput, no silent failures at scale
- Blocks: no throughput benchmarks, no evidence of memory stability at volume
- Adoption trigger: reproducible throughput benchmark + one week production evidence
- Severity: Hard (orchestration is caller's problem; needs evidence, not features)

**Ingrid (Enterprise / Regulated Environment)**
- Financial services compliance video
- Root need: verifiable, reproducible output with audit trail
- Blocks: no versioned API stability contract, exit code contract URL is TODO, no output hash in --format json
- Adoption trigger: semver stability commitment + output file hash in JSON result
- Severity: Hard (hash is easy; stability contract is a product decision we haven't made)

**Dev (SDK / Library Consumer)**
- B2B mobile video SDK (iOS + Android cross-platform)
- Root need: compile to mobile targets, direct frame access without full pipeline
- Blocks: no iOS/Android CI, no documented in-memory demuxer pattern, no lightweight decode-frames API
- Adoption trigger: working aarch64-apple-ios build + documented Cursor<Vec<u8>> usage
- Severity: Hard (architecture supports it; platform investment is large)

**Mei (Post-Production Quality Engineering)**
- QC engineer verifying delivery specs for streaming platforms
- Root need: measurable conformance (LUFS, color range, A/V sync) not just "no error"
- Blocks: no LUFS measurement, no loudness normalization (--volume is gain, not loudness), probe doesn't report measured vs declared values
- Adoption trigger: LUFS measurement in probe --format json + loudness normalization in process
- Severity: Very hard (ITU-R BS.1770 implementation is a meaningful feature area)

**Tobias (Live / Streaming Infrastructure)**
- Sports broadcaster, RTMP in / HLS out, real-time
- Root need: real-time processing of unbounded streams with segment-aware output
- Blocks: fundamentally file-to-file architecture; Demuxer requires Seek; no fMP4 segments; no RTMP/SRT input
- Adoption trigger: none foreseeable — would require a parallel streaming design
- Severity: Currently impossible without architectural changes

## Key Insight from This Analysis

Every stress-test persona evaluates a new tool by running one test they EXPECT IT TO FAIL. They're looking for honesty about limitations, not features. The `allow_color_conversion` flag is the right precedent — we should create more "we know you might be wrong about this" surfaces.

## How to apply
- Ravi and Ingrid: reachable in 6–9 months post-1.0, no architectural changes needed
- Dev: reachable with platform investment (iOS/Android CI is expensive to maintain)
- Mei: long-term roadmap, not near-term backlog
- Tobias: defines the hard outer boundary of what splica v0.1 explicitly is not — state this publicly
