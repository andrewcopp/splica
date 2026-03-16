---
name: conduct-focus-group
description: Run a persona focus group at the end of a sprint. Sam (product-lead agent) demos the current product to all 10 personas (5 core + 5 stress-test), collects feedback, synthesizes findings, and updates Notion (feedback round) and Linear (new issues). Use at sprint boundaries or when the team needs user-perspective input on the current state of splica.
disable-model-invocation: true
context: fork
agent: product-lead
---

# Conduct Focus Group

You are running a focus group for splica. This is **Round $ARGUMENTS** (if a number was provided) or the next sequential round (check your Notion Feedback Rounds index for the latest).

## Persona Tiers

splica serves two tiers of personas with different evaluation frames:

**Core 5 (friendly adopters — already engaged, expect consistency):**
These personas are using or actively evaluating splica. Their feedback is about what shipped this sprint. Regressions or broken workflows they report are P0.

**Stress-test 5 (skeptical evaluators — harder to please, expose adoption gaps):**
These personas represent the broader market splica must eventually win. Their feedback is about overall product trajectory. They inform the roadmap, not the immediate sprint backlog — unless a finding is low-effort to address.

**The contract:** Never break what the Core 5 rely on to serve a Stress-test 5 need.

## Step 1: Understand what shipped

Before showing anything to the personas, understand what's new. Do all of the following:

1. Read the recent git log to see what was committed since the last focus group:
   ```
   git log --oneline -20
   ```
2. Check Linear for the current sprint milestone — what issues were completed?
3. Check your agent memory for the last recorded codebase state.
4. Run `splica --help` to see the current CLI surface.
5. Read `crates/splica-wasm/src/lib.rs` to see what's exposed for WASM.
6. Skim `crates/splica-core/src/traits.rs` for the current trait surface.

Assemble a short "demo script" — a bulleted list of what you'll show each persona.

## Step 2: Core 5 interviews

Role-play each Core persona **one at a time**. For each persona, read their full persona file from `.claude/personas/` to get into character. Then:

1. **Show them what shipped.** Walk through the demo script from their perspective. Actually run CLI commands where relevant (`splica probe`, `splica --help`, etc.) to ground the feedback in reality.

2. **Check for regressions.** Has anything they previously relied on changed or broken? This is the most important question for Core personas.

3. **Collect their reaction.** In character, respond to what you see. What's exciting? What's frustrating? What's missing? What's confusing? Be specific — reference actual commands, error messages, API shapes, or missing features.

4. **Ask their priority question.** Each persona gets one "If you could change one thing, what would it be?" answer.

The Core 5 personas:
- **Jordan** (CLI Scripter) — `.claude/personas/cli-scripter.md`
- **Priya** (Platform Engineer) — `.claude/personas/platform-engineer.md`
- **Marcus** (App Developer) — `.claude/personas/app-developer.md`
- **Elena** (Broadcast Professional) — `.claude/personas/broadcast-professional.md`
- **Alex** (Toolchain Developer) — `.claude/personas/toolchain-developer.md`

## Step 3: Stress-test 5 interviews

Role-play each Stress-test persona **one at a time**. Read their profiles from your agent memory (`project_stress_test_personas.md`). These personas evaluate differently — they're looking for reasons NOT to adopt.

1. **Show them the full product.** Not just what shipped this sprint — the whole thing. They're evaluating whether splica is ready for their world, not whether this sprint was productive.

2. **Let them probe for weaknesses.** Each stress-test persona has a specific test they expect splica to fail. Let them run it. Honest failures build more trust than hidden gaps.

3. **Collect their verdict.** In character: would they adopt today? What's the single blocker? How far away does it feel? Be brutally honest — these are skeptical professionals, not friendly early adopters.

4. **Ask their threshold question.** "What would need to be true for you to run a production pilot?"

The Stress-test 5 personas:
- **Ravi** (High-Volume Ops) — needs predictable throughput at scale, no silent failures
- **Ingrid** (Enterprise/Regulated) — needs verifiable reproducible output, audit trail, stability contract
- **Dev** (Mobile SDK Consumer) — needs iOS/Android targets, direct frame access without full pipeline
- **Mei** (QC Engineering) — needs measurable conformance (LUFS, color range, A/V sync)
- **Tobias** (Live Streaming) — needs real-time unbounded streams; defines splica's hard outer boundary

## Step 4: Synthesize

After all 10 personas have given feedback, step back into your role as Sam and synthesize:

1. **Core 5 regressions.** Did anything break for the Core 5? These are P0 and go into the next sprint unconditionally.

2. **Core 5 themes.** What did multiple Core personas agree on? These are highest-signal items for the immediate sprint.

3. **Stress-test 5 trajectory.** Are any of the Stress-test personas getting closer to adoption? Which ones moved? Which ones are still blocked by the same thing as last round?

4. **Cross-tier themes.** Where do Core and Stress-test personas agree? These are the highest-leverage items — they satisfy existing users AND move toward broader adoption.

5. **Tensions.** Where do tiers or personas disagree? These require a product decision.

6. **Surprises.** What feedback didn't you expect? These reveal blind spots.

7. **Top 5 priorities.** Rank the top 5 feedback items by cross-persona value. Items that serve both tiers rank highest. Core 5-only items rank above Stress-test 5-only items.

## Step 5: Update Notion

Create a new Feedback Round page under the Feedback Rounds index in Notion.

**Page title format:** `Round N — [Brief description of what shipped]`

**Page structure:**
```
## What We Showed
[Bulleted list of features/changes demoed]

## Core 5 Feedback

### Jordan (CLI Scripter)
[Their reaction, regression check, specific quotes, priority request]

### Priya (Platform Engineer)
[Their reaction, regression check, specific quotes, priority request]

### Marcus (App Developer)
[Their reaction, regression check, specific quotes, priority request]

### Elena (Broadcast Professional)
[Their reaction, regression check, specific quotes, priority request]

### Alex (Toolchain Developer)
[Their reaction, regression check, specific quotes, priority request]

## Stress-test 5 Feedback

### Ravi (High-Volume Ops)
[Their verdict, blocker, distance to adoption, threshold answer]

### Ingrid (Enterprise/Regulated)
[Their verdict, blocker, distance to adoption, threshold answer]

### Dev (Mobile SDK Consumer)
[Their verdict, blocker, distance to adoption, threshold answer]

### Mei (QC Engineering)
[Their verdict, blocker, distance to adoption, threshold answer]

### Tobias (Live Streaming)
[Their verdict, blocker, distance to adoption, threshold answer]

## Synthesis

### Core 5 Regressions
[Any regressions found — or "None this round"]

### Cross-Tier Themes
[Items both tiers agree on — highest leverage]

### Core 5 Themes
[What the Core 5 want next]

### Stress-test 5 Trajectory
[Who moved closer, who didn't, why]

### Tensions
[Where personas or tiers disagree and the tradeoff]

### Surprises
[Unexpected feedback]

### Top 5 Priorities
[Ranked list with cross-persona justification, noting which tier(s) each serves]
```

## Step 6: Update Linear

For each item in your Top 5 priorities, create a Linear issue in the Splica team under the **next sprint milestone** (create the milestone if it doesn't exist yet). Follow the project conventions from your agent memory:

- Assign to `splica v0.1` project
- Assign to the next sprint milestone
- Add appropriate domain label (`core-infra`, `codec`, `container`, `dx`)
- Write the description following the issue template pattern:
  - **Problem** — what user need is unmet (reference the personas who raised it, noting tier)
  - **What's in scope** — concrete deliverables
  - **Acceptance criteria** — in terms of user outcomes, not implementation details

Also create any tech debt issues that Dana (tech lead) identified, if a tech debt review was conducted this sprint. Tag these with appropriate labels and note they came from the tech debt review.

## Step 7: Update your memory

Record in your agent memory:
- The new sprint milestone number and what's in it
- Any new product decisions made during synthesis
- Updated codebase state (what's implemented now)
- Update the Notion workspace structure if new pages were added
- Stress-test 5 trajectory updates (who moved, who didn't)

## Output

Return a summary to the user containing:
1. The Notion page URL for the feedback round
2. Any Core 5 regressions found (P0 — call these out prominently)
3. The Linear issue identifiers created (e.g., SPL-54, SPL-55, ...)
4. The top 5 priorities in one-line form, noting which tier(s) each serves
5. Stress-test 5 trajectory summary (one line per persona: closer / same / further)
6. Any product decisions made during synthesis
