# Reliability: Adversarial Input Handling (SPL-182)

This document describes each adversarial fixture scenario, the expected splica
behavior, and how ffmpeg typically handles the same input for comparison.

## 1. Truncated mdat (`truncated-mdat.mp4`)

**Input:** Valid MP4 header (ftyp + moov) but the mdat box is cut off at 70% of
its declared size. The container metadata is valid, but sample data is incomplete.

**Expected splica behavior:**
- Exit code 1 (`bad_input`)
- Structured JSON error with `error_kind` field
- Human-readable message indicating the file is truncated or unreadable

**Typical ffmpeg behavior:**
ffmpeg reads mdat lazily and will begin decoding successfully. When it reaches the
truncation point, it emits a warning like `moov atom not found` or
`Invalid data found when processing input` and exits with a non-zero code, but the
error message is unstructured (plain text to stderr) and the exit code is generic.

## 2. Zero-duration track (`zero-duration-track.mp4`)

**Input:** Valid MP4 structure with a video track where mvhd, tkhd, and mdhd
duration fields are all set to 0.

**Expected splica behavior:**
- Exit code 1 (`bad_input`)
- Structured JSON error indicating the file has no valid content or an invalid track
- The demuxer should reject the file rather than silently producing empty output

**Typical ffmpeg behavior:**
ffmpeg may accept the file and report duration as 0 or N/A. It often produces a
valid (but empty) output file without any error, which can silently break downstream
pipelines that expect non-zero duration.

## 3. Empty file (`empty-file.mp4`)

**Input:** A completely empty file (0 bytes) with an `.mp4` extension.

**Expected splica behavior:**
- Exit code 1 (`bad_input`)
- Structured JSON error with `error_kind: "bad_input"`
- Clear message like "file too small to detect format"

**Typical ffmpeg behavior:**
ffmpeg reports `Invalid data found when processing input` and exits with code 1.
The error message is generic and does not distinguish between an empty file, a
corrupt file, and an unsupported format.

## 4. Truncated header (`truncated-header.mp4`)

**Input:** Only the first 8 bytes of an ftyp box. The declared box size is 20
bytes but the file ends at byte 8.

**Expected splica behavior:**
- Exit code 1 (`bad_input`)
- Structured JSON error indicating the header is incomplete
- The demuxer should detect the truncation immediately without hanging

**Typical ffmpeg behavior:**
ffmpeg reports `moov atom not found` and exits with a non-zero code. The error
message does not clearly indicate that the file is truncated (as opposed to merely
missing a moov box).

## 5. No tracks MKV (`no-tracks.mkv`)

**Input:** Valid EBML header with `matroska` DocType and a Segment element
containing an Info element, but no Tracks element at all.

**Expected splica behavior:**
- Exit code 1 (`bad_input`)
- Structured JSON error indicating the container has no tracks
- The demuxer should fail fast rather than searching the entire file

**Typical ffmpeg behavior:**
ffmpeg may report `Could not find codec parameters` or `Stream mapping` errors.
The error message is spread across multiple lines of stderr and does not clearly
indicate the root cause (missing Tracks element). Exit code is 1 but without
structured error metadata.

---

## Design principles

1. **Structured errors over raw text.** Every error path produces a JSON object
   with `type: "error"`, `error_kind`, and `message` fields when `--format json`
   is used. This enables reliable automation without regex parsing.

2. **Deterministic exit codes.** Exit code 1 means bad input (do not retry),
   exit code 2 means internal error (may retry), exit code 3 means resource
   exhaustion (retry after backoff). Code 101 (Rust panic) is never expected.

3. **Fail fast.** Malformed input is rejected as early as possible, before
   allocating buffers or opening output files.

4. **Human-readable messages.** Error messages describe what went wrong in
   terms the user can act on, not internal implementation details.
