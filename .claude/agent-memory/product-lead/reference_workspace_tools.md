---
name: Workspace Tools — Notion and Linear
description: Notion hub structure, Linear conventions, triage workflow, and sprint report rules
type: reference
---

## Notion Workspace

Hub page: https://www.notion.so/31f1326e510281df9ce1cddebcb5c747

```
splica (hub)
├── Product North Star
├── Roadmap — phase-level only
├── Decisions — database (data source ID: c4d6254c-f484-4605-9dfb-fabbdbb84b96)
├── Personas
├── Feedback Rounds — index; child pages per round
│   ├── Rounds 1–8 (prior entries)
│   ├── Round 9 — Post-Sprint 8 (AAC/Opus Encode, WASM Decode API, process Command, Color Passthrough, Typed Errors)
│   ├── Round 10 — Post-Sprint 9 (WebM WASM Packets, Structured Progress, VolumeFilter, Color Contract, Type-Safety Bundle)
│   ├── Round 11 — Post-Sprint 10 (--volume Fix, VP9 Codec String, videoDecoderConfig Discrimination)
│   ├── Round 12 — Post-Sprint 11 (H.265 Decode + Color Passthrough, NDJSON Error Events, trim --format json, WASM Seeking, Unsupported-Codec Errors)
│   ├── Round 13 — Post-Sprint 19 (AV1 CLI Flag, WasmMkvDemuxer, JSON Contract, mod.rs Split)
│   ├── Round 14 — Post-Sprint 20 Focus Group (Benchmark Demo Sprint Planning)
│   └── Rounds 15–18 — see focus-group-rounds.md
└── Retrospectives — index; child pages per sprint (Sprint 1–17 + template)
    ├── Sprint Report Template (codifies feature + debt sprint formats)
    ├── Sprint 12–17 Reports (MKV, AV1, H.265, Debt, migrate, WASM)
    └── Sprint 21 Report — Benchmark Demos [stub, complete at sprint close]
```

**Living state** (update in place): North Star, Roadmap, Decisions DB, hub "Current focus" callout.
**Historical record** (new pages only, never edit old): Feedback Rounds, Retrospectives.
Three [ARCHIVED] pages exist from old structure — do not use.

## Linear Workspace

**Team:** Splica (SPL) | **Project:** splica v0.1

**Milestones:** Phase 0 through Sprint 28 complete. Sprint 29 = planning (last feature sprint before debt).

**Labels** (domain-based only): `core-infra`, `codec`, `container`, `dx`

**Issue template:** https://linear.app/splica/document/issue-template-73f83bc8aac3

**Conventions:** Always assign to splica v0.1 project + current sprint milestone + at least one domain label. Use blocks/blocked-by for dependencies. No estimates, no target dates yet.

**Deferred:** Cycles, sub-issues, estimates, Views/Initiatives/Triage/SLAs.

## Triage Workflow

1. Ask engineer to investigate codebase — don't guess at code readiness.
2. Record findings in Linear as "Sam's triage note" comment: what was reviewed, ready-to-start status, implementation guidance, risks.
3. Move issues to Todo when confirmed ready.
4. Update Notion if findings affect broader product picture.

## Sprint Report Format

Two variants (codified in Notion template):
- **Feature sprint:** Product Thesis Check (Moved/No movement/Blocked per persona) + What Shipped + What Didn't Ship + Single Biggest Gap
- **Debt sprint:** What Was Resolved + Structural Health (line counts before/after) + What This Unlocks + Single Biggest Remaining Risk

Rule: report written at sprint close, before next sprint planning. Never edited after the fact. "Moved" means a root need got materially closer to satisfied — not that a feature shipped.
