#!/usr/bin/env bash
# SPL-139: Structured error handling demo — splica vs ffmpeg
set -euo pipefail

SPLICA="$1"
FIXTURES="$2"
RESULTS="$3"
OUT="$RESULTS/error_handling.md"
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

cat > "$OUT" << 'HEADER'
# Error Handling: splica vs ffmpeg

Demonstrates how each tool communicates errors to automation scripts.

HEADER

# --- Test 1: Missing input file ---
echo '## 1. Missing Input File' >> "$OUT"
echo '' >> "$OUT"

echo '**ffmpeg:**' >> "$OUT"
echo '```' >> "$OUT"
set +e
ffmpeg -i nonexistent.mp4 output.mp4 >> "$OUT" 2>&1
FFMPEG_EXIT=$?
set -e
echo '```' >> "$OUT"
echo "Exit code: $FFMPEG_EXIT" >> "$OUT"
echo '' >> "$OUT"

echo '**splica:**' >> "$OUT"
echo '```' >> "$OUT"
set +e
"$SPLICA" process --input nonexistent.mp4 --output output.mp4 >> "$OUT" 2>&1
SPLICA_EXIT=$?
set -e
echo '```' >> "$OUT"
echo "Exit code: $SPLICA_EXIT (1 = bad input, don't retry)" >> "$OUT"
echo '' >> "$OUT"

# --- Test 2: Invalid/corrupt input ---
echo '## 2. Invalid Input (not a media file)' >> "$OUT"
echo '' >> "$OUT"

echo "this is not a media file" > "$TMP_DIR/fake.mp4"

echo '**ffmpeg:**' >> "$OUT"
echo '```' >> "$OUT"
set +e
ffmpeg -i "$TMP_DIR/fake.mp4" "$TMP_DIR/out.mp4" >> "$OUT" 2>&1
FFMPEG_EXIT=$?
set -e
echo '```' >> "$OUT"
echo "Exit code: $FFMPEG_EXIT" >> "$OUT"
echo '' >> "$OUT"

echo '**splica:**' >> "$OUT"
echo '```' >> "$OUT"
set +e
"$SPLICA" process --input "$TMP_DIR/fake.mp4" --output "$TMP_DIR/out.mp4" >> "$OUT" 2>&1
SPLICA_EXIT=$?
set -e
echo '```' >> "$OUT"
echo "Exit code: $SPLICA_EXIT" >> "$OUT"
echo '' >> "$OUT"

# --- Test 3: JSON error output ---
echo '## 3. Structured JSON Error Output' >> "$OUT"
echo '' >> "$OUT"

echo '**ffmpeg** has no structured error output. You must parse stderr text.' >> "$OUT"
echo '' >> "$OUT"

echo '**splica** with `--format json`:' >> "$OUT"
echo '```json' >> "$OUT"
set +e
"$SPLICA" process --input nonexistent.mp4 --output output.mp4 --format json >> "$OUT" 2>&1
set -e
echo '```' >> "$OUT"
echo '' >> "$OUT"

# --- Test 4: Exit code contract ---
echo '## 4. Exit Code Contract' >> "$OUT"
echo '' >> "$OUT"
cat >> "$OUT" << 'EOF'
| Exit Code | Meaning | Action |
|-----------|---------|--------|
| 0 | Success | Continue |
| 1 | Bad input (invalid file, unsupported codec) | Don't retry — fix input |
| 2 | Internal error (codec crash, unexpected state) | Retry may work |
| 3 | Resource exhaustion (out of memory, disk full) | Retry with different limits |

ffmpeg uses exit code 1 for everything. Automation scripts must grep stderr
to distinguish between "file not found" and "out of disk space" — and the
stderr format is not guaranteed stable between versions.

### Example: Automated retry logic

```bash
# With splica:
splica process --input "$file" --output "$out" --format json
case $? in
    0) echo "Success" ;;
    1) echo "Bad input: skip $file" ;;
    2) echo "Internal error: retry" && retry "$file" ;;
    3) echo "Resources: retry with lower quality" && retry_low "$file" ;;
esac

# With ffmpeg:
ffmpeg -i "$file" "$out" 2>stderr.log
if [ $? -ne 0 ]; then
    if grep -q "No such file" stderr.log; then
        echo "Bad input"
    elif grep -q "Cannot allocate" stderr.log; then
        echo "Resources"
    else
        echo "Unknown error"  # hope for the best
    fi
fi
```
EOF
echo '' >> "$OUT"

echo "  -> $OUT"
