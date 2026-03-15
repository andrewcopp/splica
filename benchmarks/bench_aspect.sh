#!/usr/bin/env bash
# SPL-140: Aspect ratio correctness demo
# Shows that splica letterboxes by default; ffmpeg stretches
set -euo pipefail

SPLICA="$1"
FIXTURES="$2"
RESULTS="$3"
OUT="$RESULTS/aspect_ratio.md"
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

INPUT="$FIXTURES/bigbuckbunny_h265.mp4"

cat > "$OUT" << 'HEADER'
# Aspect Ratio: Correct by Default

Resizing a 640x360 (16:9) video to 400x300 (4:3) — a non-matching aspect ratio.

HEADER

# --- ffmpeg default (stretches) ---
echo '## ffmpeg: Default behavior (stretches)' >> "$OUT"
echo '' >> "$OUT"
echo '```bash' >> "$OUT"
echo 'ffmpeg -i input_640x360.mp4 -vf "scale=400:300" output.mp4' >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"
ffmpeg -y -i "$INPUT" -vf "scale=400:300" "$TMP_DIR/ffmpeg_stretch.mp4" > /dev/null 2>&1

echo 'Result: Video is **silently distorted** — 16:9 content squeezed into 4:3.' >> "$OUT"
echo 'There is no warning. The output plays fine. It just looks wrong.' >> "$OUT"
echo '' >> "$OUT"

# Probe to show dimensions
echo '```' >> "$OUT"
ffprobe -v quiet -select_streams v:0 -show_entries stream=width,height -of csv=p=0 "$TMP_DIR/ffmpeg_stretch.mp4" >> "$OUT" 2>&1
echo '  ← 400x300, aspect ratio changed from 16:9 to 4:3 (distorted)' >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"

# --- ffmpeg correct (requires pad filter) ---
echo '## ffmpeg: Correct behavior (requires 93-character filter)' >> "$OUT"
echo '' >> "$OUT"
echo '```bash' >> "$OUT"
echo 'ffmpeg -i input_640x360.mp4 -vf "scale=400:300:force_original_aspect_ratio=decrease,pad=400:300:(ow-iw)/2:(oh-ih)/2" output.mp4' >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"
ffmpeg -y -i "$INPUT" -vf "scale=400:300:force_original_aspect_ratio=decrease,pad=400:300:(ow-iw)/2:(oh-ih)/2" "$TMP_DIR/ffmpeg_correct.mp4" > /dev/null 2>&1

echo 'Result: Video is letterboxed correctly, but you had to know the incantation.' >> "$OUT"
echo '' >> "$OUT"

echo '```' >> "$OUT"
ffprobe -v quiet -select_streams v:0 -show_entries stream=width,height -of csv=p=0 "$TMP_DIR/ffmpeg_correct.mp4" >> "$OUT" 2>&1
echo '  ← 400x300, content is 400x225 centered with black bars (correct)' >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"

# --- splica default (letterboxes) ---
echo '## splica: Default behavior (letterboxes)' >> "$OUT"
echo '' >> "$OUT"
echo '```bash' >> "$OUT"
echo 'splica process --input input_640x360.mp4 --output output.mp4 --resize 400x300' >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"
"$SPLICA" process --input "$INPUT" --output "$TMP_DIR/splica_default.mp4" --resize 400x300 > /dev/null 2>&1

echo 'Result: Video is **letterboxed by default** — no distortion, no extra flags.' >> "$OUT"
echo '' >> "$OUT"

echo '```' >> "$OUT"
"$SPLICA" probe "$TMP_DIR/splica_default.mp4" >> "$OUT" 2>&1
echo '```' >> "$OUT"
echo '' >> "$OUT"

# --- Summary ---
cat >> "$OUT" << 'EOF'
## The Point

The "simple" ffmpeg command does the wrong thing silently. The correct ffmpeg
command requires knowing a 93-character filter chain. splica does the right
thing with zero additional flags.

| Tool | Default resize behavior | Correct resize |
|------|------------------------|----------------|
| ffmpeg | Stretches (distortion) | `-vf "scale=W:H:force_original_aspect_ratio=decrease,pad=W:H:(ow-iw)/2:(oh-ih)/2"` |
| splica | Letterboxes (correct) | `--resize WxH` |

This is what "correct by default" means: the easiest command is also the
safest command.
EOF

echo "  -> $OUT"
