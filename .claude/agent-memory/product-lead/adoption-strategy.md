# Adoption Strategy

## Core Philosophy (2026-03-11)

**Ship when there's pull, not push.**

Personas don't see the vision yet — they only see the promise of a credible ffmpeg alternative. The job right now is to prove that core promise. Adoption-expansion features come after product-market fit is demonstrated, not before.

Do not tie adoption initiatives to specific sprints. Execute them when real persona adoption creates signal that the core is working.

## Trigger Condition

Real persona adoption / product-market fit signal. Concretely: multiple personas (not just one) are using splica in production or near-production workflows and recommending it unprompted as an ffmpeg alternative.

## Two Initiatives — Parked Until Trigger

### 1. Agent CLI Layer

A natural language interface that translates intent into splica commands.

**What it is (when built):**
- Not a simplified parallel CLI — that fragments the surface
- An agent that emits visible, copyable `splica` commands and shows its work
- Always prints the command before executing; always supports `--dry-run`
- Teaches the CLI rather than hiding it — builds Jordan's mental model over time
- Could describe pipeline behavior in plain language and emit shell scripts with correct exit code handling (Priya unlock)

**Prerequisite before building:** CLI audit must happen first. The agent is a multiplier on the CLI surface — if the surface has rough edges, the agent makes them invisible and unfixable.

**Why not now:** We'd be building adoption features for users we don't have yet. The agent amplifies a good CLI; it can't substitute for one.

### 2. Reliability Comparison

An honest, documented comparison of splica vs. ffmpeg on failure behavior — not raw throughput benchmarks.

**What it is (when built):**
- A table: scenario / ffmpeg behavior / splica behavior
- Rows: unsupported codec, container/codec mismatch, truncated input, color space mismatch, etc.
- A real case study: a pathological input that causes ffmpeg silent corruption, and what splica does instead (correct output or loud failure)
- A published error behavior contract: every failure mode, exit code, JSON shape — readable by a skeptical engineer

**What it is NOT:**
- Raw transcode speed benchmarks — ffmpeg wins those and it's not our pitch
- A marketing claim about being "faster" — sets up an unwinnable arms race

**Prerequisites before publishing:**
- Encode matrix complete (H.265 encode + AV1 encode landed)
- Error contract documented publicly
- Real pathological scenario researched and tested

**Why not now:** We have encode matrix holes. Publishing a reliability comparison before H.265 encode lands invites the obvious rebuttal that we can't handle the full common-format matrix.
