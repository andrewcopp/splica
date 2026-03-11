---
name: sprint-report
description: Generate an end-of-sprint stakeholder update summarizing what was built, what debt was paid down, product readiness assessment, and distance to an adoptable product. Use at the end of each sprint cycle after the focus group and tech debt review are complete.
disable-model-invocation: true
argument-hint: "[sprint-number]"
---

# Sprint Report

Generate a stakeholder update for Sprint $ARGUMENTS (or the current sprint if no number given).

## Step 1: Gather the facts

Collect data from all available sources. Do all of the following:

1. **Git log** — what commits landed this sprint? Count commits, files changed, lines added/removed.
   ```
   git log --oneline --since="[sprint start]"
   git diff --stat [start-commit]..HEAD
   ```

2. **Linear** — pull the current sprint milestone. List all issues with their status (Done, In Progress, Backlog). Note any that slipped from a previous sprint.

3. **Focus group** — check Notion for the most recent Feedback Round page. Pull the synthesis (cross-persona themes, top 5 priorities).

4. **Tech debt review** — check if Dana (tech-lead) produced findings this sprint. Pull the priority summary if so.

5. **Test suite** — run `cargo test` and count total tests passing. Run `cargo clippy --all-targets -- -D warnings` on changed crates.

6. **WASM health** — check if `cargo build --target wasm32-unknown-unknown -p splica-wasm --release` succeeds and note the bundle size.

7. **CLI surface** — run `splica --help` to capture the current command set.

8. **Crate health** — list all workspace crates and note which ones have real implementations vs. stubs.

## Step 2: Write the report

Structure the report as follows. Write in plain language — the audience is someone who cares about progress and product readiness, not implementation details.

```markdown
# Sprint [N] Report

**Date:** [today]
**Sprint theme:** [one-line summary of what this sprint was about]

---

## What we shipped

[3-7 bullet points. Each bullet is a user-visible capability, not an internal refactor. Frame as "you can now..." or "splica now supports..." When a capability spans multiple issues, collapse them into one bullet.]

## By the numbers

| Metric | Value |
|--------|-------|
| Issues completed | X / Y planned |
| Commits | N |
| Tests passing | N |
| WASM bundle size | N KB |
| CLI commands | [list] |
| Supported codecs | [list] |
| Supported containers | [list] |

## What we learned (focus group)

[2-3 sentences summarizing the cross-persona themes from the focus group. What are users telling us? Where is energy highest? Where is frustration?]

## Tech debt addressed

[If Dana reviewed this sprint: 2-3 sentences on what debt was identified and what was paid down. If no review: "No tech debt review this sprint."]

## What's next

[3-5 bullets on what the next sprint will focus on, pulled from Linear's next milestone.]

---

## Product Readiness Assessment

This is the core of the report. Evaluate splica's distance to an adoptable product — meaning a developer could reasonably choose splica over ffmpeg for a real project.

### Readiness by capability

Rate each capability on a four-point scale:
- **Ready** — works correctly, tested with real files, documented
- **Functional** — works for common cases, may have edge case gaps
- **Partial** — core is there but missing pieces block real use
- **Missing** — not yet implemented

| Capability | Status | Notes |
|------------|--------|-------|
| MP4 demux | ? | |
| MP4 mux | ? | |
| WebM demux | ? | |
| WebM mux | ? | |
| MKV demux/mux | ? | |
| H.264 decode | ? | |
| H.264 encode | ? | |
| H.265 decode | ? | |
| AV1 decode/encode | ? | |
| AAC decode | ? | |
| AAC encode | ? | |
| Opus decode/encode | ? | |
| Video filtering (scale, crop) | ? | |
| Audio filtering | ? | |
| CLI: probe | ? | |
| CLI: convert/transcode | ? | |
| CLI: trim | ? | |
| CLI: extract-audio | ? | |
| WASM bindings | ? | |
| Structured JSON output | ? | |
| Error handling (retryable vs fatal) | ? | |
| Real-file test coverage | ? | |

### The honest assessment

Write 2-3 paragraphs answering:

1. **Where are we?** What can splica actually do today that a developer would use in production? Be specific — not "it can process video" but "it can demux an MP4, probe its tracks, and remux to WebM with correct timing."

2. **What's blocking adoption?** What are the 2-3 biggest gaps between current state and a product someone would choose over ffmpeg for even simple tasks? Be honest about what's missing.

3. **How far out?** Given the current pace and scope, roughly how many sprints until splica could handle the "convert this MP4 to WebM" use case end-to-end with confidence? What about "transcode H.264 to H.264 at a different resolution"? Don't give dates — give sprint counts and what each would need to deliver.

### Confidence level

End with a single line:

**Overall confidence: [low | growing | moderate | high]** — [one sentence justification]

This should be brutally honest. "High" means you'd recommend splica to a colleague today. "Low" means foundational work remains. Most early sprints should be "growing."
```

## Step 3: Post to Notion

Create a new page under the Retrospectives index in Notion.

**Page title format:** `Sprint [N] Report — [date]`

Post the full report as the page content.

## Step 4: Return summary

Return to the user:
1. The Notion page URL
2. The "What we shipped" bullets
3. The overall confidence level and justification
4. The top 2-3 adoption blockers
