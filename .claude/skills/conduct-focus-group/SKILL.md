---
name: conduct-focus-group
description: Run a persona focus group at the end of a sprint. Sam (product-lead agent) demos the current product to all five personas, collects feedback, synthesizes findings, and updates Notion (feedback round) and Linear (new issues). Use at sprint boundaries or when the team needs user-perspective input on the current state of splica.
disable-model-invocation: true
context: fork
agent: product-lead
---

# Conduct Focus Group

You are running a focus group for splica. This is **Round $ARGUMENTS** (if a number was provided) or the next sequential round (check your Notion Feedback Rounds index for the latest).

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

## Step 2: Conduct the focus group

Role-play each persona **one at a time**. For each persona, read their full persona file from `.claude/personas/` to get into character. Then:

1. **Show them the product.** Walk through the demo script from their perspective. Actually run CLI commands where relevant (`splica probe`, `splica --help`, etc.) to ground the feedback in reality.

2. **Collect their reaction.** In character, respond to what you see. What's exciting? What's frustrating? What's missing? What's confusing? Be specific — reference actual commands, error messages, API shapes, or missing features.

3. **Ask their priority question.** Each persona gets one "If you could change one thing, what would it be?" answer.

The five personas are:
- **Jordan** (CLI Scripter) — `.claude/personas/cli-scripter.md`
- **Priya** (Platform Engineer) — `.claude/personas/platform-engineer.md`
- **Marcus** (App Developer) — `.claude/personas/app-developer.md`
- **Elena** (Broadcast Professional) — `.claude/personas/broadcast-professional.md`
- **Alex** (Toolchain Developer) — `.claude/personas/toolchain-developer.md`

## Step 3: Synthesize

After all five personas have given feedback, step back into your role as Sam and synthesize:

1. **Cross-persona themes.** What did multiple personas agree on? These are highest signal.
2. **Tensions.** Where do personas disagree? These require a product decision.
3. **Surprises.** What feedback didn't you expect? These reveal blind spots.
4. **Top 5 priorities.** Rank the top 5 feedback items by cross-persona value — how many personas benefit and how much.

## Step 4: Update Notion

Create a new Feedback Round page under the Feedback Rounds index in Notion.

**Page title format:** `Round N — [Brief description of what shipped]`

**Page structure:**
```
## What We Showed
[Bulleted list of features/changes demoed]

## Persona Feedback

### Jordan (CLI Scripter)
[Their reaction, specific quotes, priority request]

### Priya (Platform Engineer)
[Their reaction, specific quotes, priority request]

### Marcus (App Developer)
[Their reaction, specific quotes, priority request]

### Elena (Broadcast Professional)
[Their reaction, specific quotes, priority request]

### Alex (Toolchain Developer)
[Their reaction, specific quotes, priority request]

## Synthesis

### Cross-Persona Themes
[Bulleted list]

### Tensions
[Where personas disagree and the tradeoff]

### Surprises
[Unexpected feedback]

### Top 5 Priorities
[Ranked list with cross-persona justification]
```

## Step 5: Update Linear

For each item in your Top 5 priorities, create a Linear issue in the Splica team under the **next sprint milestone** (create the milestone if it doesn't exist yet). Follow the project conventions from your agent memory:

- Assign to `splica v0.1` project
- Assign to the next sprint milestone
- Add appropriate domain label (`core-infra`, `codec`, `container`, `dx`)
- Write the description following the issue template pattern:
  - **Problem** — what user need is unmet (reference the personas who raised it)
  - **What's in scope** — concrete deliverables
  - **Acceptance criteria** — in terms of user outcomes, not implementation details

Also create any tech debt issues that Dana (tech lead) identified, if a tech debt review was conducted this sprint. Tag these with appropriate labels and note they came from the tech debt review.

## Step 6: Update your memory

Record in your agent memory:
- The new sprint milestone number and what's in it
- Any new product decisions made during synthesis
- Updated codebase state (what's implemented now)
- Update the Notion workspace structure if new pages were added

## Output

Return a summary to the user containing:
1. The Notion page URL for the feedback round
2. The Linear issue identifiers created (e.g., SPL-54, SPL-55, ...)
3. The top 5 priorities in one-line form
4. Any product decisions made during synthesis
