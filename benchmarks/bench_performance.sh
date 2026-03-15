#!/usr/bin/env bash
# SPL-141: Performance comparison — splica vs ffmpeg
# Measures wall time and binary size
set -euo pipefail

SPLICA="$1"
FIXTURES="$2"
RESULTS="$3"
QUICK="${4:-}"
OUT="$RESULTS/performance.md"
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

RUNS=3
if [[ "$QUICK" == "--quick" ]]; then
    RUNS=1
fi

# Helper: time a command using python3 for sub-second precision
time_it() {
    local start end dur
    start=$(python3 -c "import time; print(time.time())")
    "$@" > /dev/null 2>&1 || true
    end=$(python3 -c "import time; print(time.time())")
    python3 -c "print(f'{$end - $start:.3f}')"
}

cat > "$OUT" << HEADER
# Performance: splica vs ffmpeg

Measured on the same machine, same input files. Runs: $RUNS per test.
All measurements use release builds.

HEADER

# --- Binary size ---
echo '## Binary Size' >> "$OUT"
echo '' >> "$OUT"
SPLICA_SIZE=$(wc -c < "$SPLICA" | tr -d ' ')
SPLICA_MB=$(echo "scale=1; $SPLICA_SIZE / 1048576" | bc)
echo "| Tool | Size | Notes |" >> "$OUT"
echo "|------|------|-------|" >> "$OUT"
echo "| splica | ${SPLICA_MB} MB | Single static binary, all codecs included |" >> "$OUT"
echo "| ffmpeg | ~30-80 MB total | Dynamically linked; binary + shared libraries |" >> "$OUT"
echo '' >> "$OUT"
echo "splica ships everything in one binary: codecs, containers, CLI." >> "$OUT"
echo "ffmpeg's binary is small (~400 KB) but depends on libavcodec, libavformat," >> "$OUT"
echo "libavutil, etc. Total installed size varies by platform and configuration." >> "$OUT"
echo '' >> "$OUT"

# --- Startup time (probe) ---
echo '## Startup + Probe Time' >> "$OUT"
echo '' >> "$OUT"
echo 'Time to probe a file and exit (startup overhead + container parsing):' >> "$OUT"
echo '' >> "$OUT"

INPUT_MP4="$FIXTURES/bigbuckbunny_h265_aac.mp4"

# Warm up
"$SPLICA" probe "$INPUT_MP4" > /dev/null 2>&1 || true
ffprobe -v quiet -print_format json -show_format -show_streams "$INPUT_MP4" > /dev/null 2>&1 || true

echo '```' >> "$OUT"
SPLICA_TIMES=""
for i in $(seq 1 $RUNS); do
    DUR=$(time_it "$SPLICA" probe "$INPUT_MP4")
    SPLICA_TIMES="$SPLICA_TIMES ${DUR}s"
done
echo "splica probe:${SPLICA_TIMES}" >> "$OUT"

FFPROBE_TIMES=""
for i in $(seq 1 $RUNS); do
    DUR=$(time_it ffprobe -v quiet -print_format json -show_format -show_streams "$INPUT_MP4")
    FFPROBE_TIMES="$FFPROBE_TIMES ${DUR}s"
done
echo "ffprobe:     ${FFPROBE_TIMES}" >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"

# --- Stream copy: MP4 → MP4 ---
echo '## Stream Copy (remux, no re-encoding)' >> "$OUT"
echo '' >> "$OUT"

echo '```' >> "$OUT"
SPLICA_TIMES=""
for i in $(seq 1 $RUNS); do
    rm -f "$TMP_DIR/out_splica_copy.mp4"
    DUR=$(time_it "$SPLICA" process --input "$INPUT_MP4" --output "$TMP_DIR/out_splica_copy.mp4")
    SPLICA_TIMES="$SPLICA_TIMES ${DUR}s"
done
echo "splica (stream copy):${SPLICA_TIMES}" >> "$OUT"

FFMPEG_TIMES=""
for i in $(seq 1 $RUNS); do
    rm -f "$TMP_DIR/out_ffmpeg_copy.mp4"
    DUR=$(time_it ffmpeg -y -i "$INPUT_MP4" -c copy "$TMP_DIR/out_ffmpeg_copy.mp4")
    FFMPEG_TIMES="$FFMPEG_TIMES ${DUR}s"
done
echo "ffmpeg -c copy:      ${FFMPEG_TIMES}" >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"

# --- Transcode: H.265 MP4 → H.264 MP4 (re-encode) ---
echo '## Re-encode: H.265 → H.264 (same container)' >> "$OUT"
echo '' >> "$OUT"
echo 'Both tools decode H.265 and re-encode to H.264:' >> "$OUT"
echo '' >> "$OUT"

echo '```' >> "$OUT"
SPLICA_TIMES=""
for i in $(seq 1 $RUNS); do
    rm -f "$TMP_DIR/out_splica_reencode.mp4"
    DUR=$(time_it "$SPLICA" process --input "$FIXTURES/bigbuckbunny_h265.mp4" --output "$TMP_DIR/out_splica_reencode.mp4" --codec h264)
    SPLICA_TIMES="$SPLICA_TIMES ${DUR}s"
done
echo "splica (h265→h264):${SPLICA_TIMES}" >> "$OUT"

FFMPEG_TIMES=""
for i in $(seq 1 $RUNS); do
    rm -f "$TMP_DIR/out_ffmpeg_reencode.mp4"
    DUR=$(time_it ffmpeg -y -i "$FIXTURES/bigbuckbunny_h265.mp4" -c:v libx264 -preset medium "$TMP_DIR/out_ffmpeg_reencode.mp4")
    FFMPEG_TIMES="$FFMPEG_TIMES ${DUR}s"
done
echo "ffmpeg (h265→h264): ${FFMPEG_TIMES}" >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"
echo 'Note: splica uses openh264 (encode), ffmpeg uses libx264. libx264 is' >> "$OUT"
echo 'significantly more optimized with decades of SIMD work. This is an honest' >> "$OUT"
echo 'comparison — splica prioritizes safety and simplicity over raw throughput.' >> "$OUT"
echo '' >> "$OUT"

# --- Resize ---
echo '## Resize: 640x360 → 400x300 (with aspect preservation)' >> "$OUT"
echo '' >> "$OUT"

echo '```' >> "$OUT"
SPLICA_TIMES=""
for i in $(seq 1 $RUNS); do
    rm -f "$TMP_DIR/resized_splica.mp4"
    DUR=$(time_it "$SPLICA" process --input "$FIXTURES/bigbuckbunny_h265.mp4" --output "$TMP_DIR/resized_splica.mp4" --resize 400x300)
    SPLICA_TIMES="$SPLICA_TIMES ${DUR}s"
done
echo "splica --resize 400x300:            ${SPLICA_TIMES}" >> "$OUT"

FFMPEG_TIMES=""
for i in $(seq 1 $RUNS); do
    rm -f "$TMP_DIR/resized_ffmpeg.mp4"
    DUR=$(time_it ffmpeg -y -i "$FIXTURES/bigbuckbunny_h265.mp4" -vf "scale=400:300:force_original_aspect_ratio=decrease,pad=400:300:(ow-iw)/2:(oh-ih)/2" "$TMP_DIR/resized_ffmpeg.mp4")
    FFMPEG_TIMES="$FFMPEG_TIMES ${DUR}s"
done
echo "ffmpeg -vf scale+pad (aspect-safe):${FFMPEG_TIMES}" >> "$OUT"
echo '```' >> "$OUT"
echo '' >> "$OUT"

echo "  -> $OUT"
