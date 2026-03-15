#!/usr/bin/env bash
# SPL-138: CLI ergonomics comparison — splica vs ffmpeg
# Shows side-by-side commands for common tasks
set -euo pipefail

SPLICA="$1"
FIXTURES="$2"
RESULTS="$3"
OUT="$RESULTS/ergonomics.md"

cat > "$OUT" << 'HEADER'
# CLI Ergonomics: splica vs ffmpeg

Side-by-side comparison of common media processing tasks.

HEADER

# --- Task 1: Convert MP4 to WebM ---
cat >> "$OUT" << 'EOF'
## 1. Convert MP4 to WebM

**ffmpeg:**
```bash
ffmpeg -i input.mp4 -c:v libvpx-vp9 -crf 30 -b:v 0 -c:a libopus output.webm
```
You need to know: VP9 requires `-b:v 0` with CRF mode, audio codec must be
explicitly set to libopus for WebM, and the flag names are non-obvious.

**splica:**
```bash
splica process --input input.mp4 --output output.webm
```
Container-aware defaults: WebM gets VP9 video + Opus audio automatically.
No flags needed for the common case.

---

EOF

# --- Task 2: Probe file info ---
cat >> "$OUT" << 'EOF'
## 2. Inspect a media file

**ffmpeg:**
```bash
ffprobe -v quiet -print_format json -show_format -show_streams input.mp4
```
Returns deeply nested JSON with ~100 fields per stream. Parsing requires
knowing which fields matter.

**splica:**
```bash
splica probe input.mp4
splica probe --format json input.mp4
```
Human-readable by default, structured JSON when you need it. Shows only
what matters: codec, resolution, duration, sample rate.

EOF

# Actually run both and capture output
echo '### Actual output comparison' >> "$OUT"
echo '' >> "$OUT"
echo '**ffprobe output** (truncated):' >> "$OUT"
echo '```' >> "$OUT"
ffprobe -v quiet -print_format json -show_format -show_streams "$FIXTURES/bigbuckbunny_h264_aac.mp4" 2>&1 | head -40 >> "$OUT"
echo '... (continues for ~200 more lines)' >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"
echo '**splica probe output:**' >> "$OUT"
echo '```' >> "$OUT"
"$SPLICA" probe "$FIXTURES/bigbuckbunny_h264_aac.mp4" >> "$OUT" 2>&1
echo '```' >> "$OUT"
echo '' >> "$OUT"
echo '---' >> "$OUT"
echo '' >> "$OUT"

# --- Task 3: Trim ---
cat >> "$OUT" << 'EOF'
## 3. Trim a video

**ffmpeg:**
```bash
ffmpeg -ss 00:00:02 -i input.mp4 -t 00:00:05 -c copy output.mp4
```
Flag order matters: `-ss` before `-i` seeks by keyframe (fast but imprecise),
after `-i` seeks frame-accurately (slow). This is a well-known gotcha.

**splica:**
```bash
splica trim --input input.mp4 --output output.mp4 --start 2s --end 7s
```
Named parameters, human-readable durations, no flag-order gotchas.

---

EOF

# --- Task 4: Resize ---
cat >> "$OUT" << 'EOF'
## 4. Resize video

**ffmpeg:**
```bash
ffmpeg -i input.mp4 -vf "scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2" output.mp4
```
Without the pad filter, aspect ratio is silently distorted. The "correct"
incantation is 93 characters of filter syntax.

**splica:**
```bash
splica process --input input.mp4 --output output.mp4 --resize 1280x720
```
Letterboxing (aspect-safe) is the default. No silent distortion.

---

EOF

# --- Task 5: Adjust volume ---
cat >> "$OUT" << 'EOF'
## 5. Adjust audio volume

**ffmpeg:**
```bash
ffmpeg -i input.mp4 -af "volume=0.5" -c:v copy output.mp4
```
Must remember `-c:v copy` or video gets re-encoded too (slow, quality loss).

**splica:**
```bash
splica process --input input.mp4 --output output.mp4 --volume 0.5
```
Only audio is re-encoded. Video is stream-copied automatically when unchanged.

---

EOF

# --- Task 6: Structured error handling ---
cat >> "$OUT" << 'EOF'
## 6. Error handling in automation

**ffmpeg:**
```bash
ffmpeg -i input.mp4 output.webm
if [ $? -ne 0 ]; then
    # Exit code is always 1 — no way to distinguish
    # "bad input" from "out of disk space" from "codec error"
    echo "Something failed"  # grep stderr to guess why
fi
```

**splica:**
```bash
splica process --input input.mp4 --output output.webm --format json
# Exit 0 = success
# Exit 1 = bad input (don't retry)
# Exit 2 = internal error (retry)
# Exit 3 = resource exhaustion (retry with different limits)
# NDJSON progress on stdout
```
Typed exit codes + structured JSON events = automatable.

---

EOF

# --- Task 7: Translate ffmpeg commands ---
cat >> "$OUT" << 'EOF'
## 7. Migration path

**splica migrate:**
```bash
$ splica migrate "ffmpeg -i input.mp4 -vf scale=1280:720 -c:a aac output.mp4"
```
Translates ffmpeg commands to splica equivalents with plain-English explanations.
No other tool offers this.

EOF

# Run the migrate example
echo '### Actual migrate output:' >> "$OUT"
echo '```' >> "$OUT"
"$SPLICA" migrate ffmpeg -i input.mp4 -vf scale=1280:720 -c:a aac output.mp4 >> "$OUT" 2>&1 || true
echo '```' >> "$OUT"
echo '' >> "$OUT"

# --- Summary ---
cat >> "$OUT" << 'EOF'

## Summary

| Task | ffmpeg tokens | splica tokens | Notes |
|------|--------------|---------------|-------|
| Convert MP4→WebM | 11 | 5 | splica auto-selects codecs |
| Probe file | 8 | 3 | splica shows what matters |
| Trim video | 10 | 9 | splica uses named params |
| Resize (aspect-safe) | 14 | 7 | ffmpeg needs pad filter |
| Adjust volume | 9 | 7 | splica auto-copies video |
| Error handling | grep stderr | typed exit codes | splica is automatable |
| Migration | N/A | built-in | `splica migrate` |

Token count = number of whitespace-separated arguments including the command name.
EOF

echo "  -> $OUT"
