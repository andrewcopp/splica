---
name: tech-debt-review
description: Review the codebase for tech debt, over/under-engineering, testing gaps, dead code, and architectural issues. Use when asked to audit code quality, pay down tech debt, or review the health of the codebase.
argument-hint: "[crate-name or 'all']"
---

# Tech Debt Review

Perform a thorough tech debt review of the splica codebase. If an argument is provided, focus on that crate (e.g., `splica-mp4`). Otherwise, review the entire workspace.

## Scope

Review the target code through each of these lenses, in order. For each lens, produce a short section with concrete findings — file paths, line numbers, and specific observations. Skip any lens that has no findings.

### 1. Over-engineering

What is more complex than it needs to be? Look for:
- Abstractions that serve only one call site
- Generic type parameters that are never varied
- Builder patterns where a simple constructor would do
- Trait hierarchies deeper than necessary
- Feature flags that gate code no one has opted out of
- Premature optimization (caching, pooling, custom allocators) without benchmarks proving need

### 2. Under-engineering

What is too simple for what it's doing? Look for:
- `unwrap()` or `expect()` in library code (not tests)
- Missing error context (bare `?` without `.map_err()` or `.context()` where the caller can't diagnose)
- Stringly-typed values that should be enums or newtypes
- Public API surfaces that expose internal details
- Missing `#[must_use]` on types where ignoring the return is always a bug

### 3. Ticking time bombs

What will become a bigger problem if not addressed now? Look for:
- `todo!()`, `unimplemented!()`, `FIXME`, `HACK`, `XXX` comments
- Hardcoded constants that should be configurable (magic numbers)
- Assumptions about data sizes, track counts, or codec behavior that real files will violate
- Missing bounds checks on user-provided indices
- Fallback values (e.g., `_ => default`) that silently swallow unexpected input

### 4. Hindsight redesigns

What would we do differently if starting today with current knowledge? Consider:
- Are error types well-structured, or did they grow organically?
- Are module boundaries in the right place?
- Is the crate dependency graph clean, or are there circular concerns?
- Are newtypes used consistently for domain concepts (timestamps, track indices)?
- Does the public API follow Rust conventions (Into, AsRef, standard trait impls)?

### 5. Testing gaps

What isn't tested that should be? Look for:
- Public functions with no test coverage
- Edge cases: empty input, single-element input, maximum values, zero-length durations
- Error paths — are error variants actually exercised in tests?
- Integration gaps — are crate boundaries tested end-to-end?
- Property-based testing opportunities (round-trip encode/decode)

### 6. Embarrassments

What would you be embarrassed to show a peer? Look for:
- Copy-pasted code blocks (DRY violations)
- Inconsistent naming conventions
- Misleading comments or stale doc comments
- Functions longer than 30 lines
- Files longer than 300 lines
- Dead parameters or unused imports that clippy didn't catch
- Commented-out code

### 7. Dead code

What can be deleted? Look for:
- Functions, structs, or modules with no callers (beyond re-exports)
- Feature-gated code where the feature is never enabled in any dependent
- Test helpers that no test uses
- Stale re-exports in `lib.rs` that downstream crates don't import

### 8. Dependency injection gaps

Where are concrete types used where traits would improve testability? Look for:
- Functions that construct their own dependencies internally
- Hardcoded I/O (direct file opens, network calls) in library code
- Types that are difficult to mock or stub in tests
- Places where `dyn Trait` or generics would decouple crates

### 9. Missing abstractions

Where would a new type or trait reduce complexity? Look for:
- Multiple functions that take the same cluster of parameters
- Match statements repeated in multiple places
- Raw byte manipulation that could be a parsing combinator or typed reader
- Similar logic across container formats (MP4/WebM/MKV) that isn't shared

## Output format

For each lens with findings, produce:

```
## [Lens Name]

### [Finding title] — `crate/path/file.rs:line`
[1-3 sentence description of the issue]
**Severity**: low | medium | high
**Effort**: small | medium | large
**Recommendation**: [What to do about it]
```

After all lenses, produce a **Priority summary** — a ranked list of the top 5 items to address first, considering severity and effort. Format as a numbered list with the finding title and a one-line justification.

## Guidelines

- Be specific. "Error handling could be better" is not a finding. "Mp4Demuxer::read_box uses bare `?` on line 47, losing the byte offset context" is.
- Calibrate severity honestly. Not everything is high. Most codebases have lots of low-severity items.
- Don't flag things that are intentional and documented (e.g., "SAFETY:" comments on unsafe, known limitations noted in doc comments).
- Read CLAUDE.md conventions before flagging style issues — some things that look like debt may be intentional project decisions.
- Focus on actionable findings. If something is technically debt but the cost of fixing exceeds the benefit, note that in the recommendation.
