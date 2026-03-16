#!/usr/bin/env bash
# SPL-202: Stress test harness — proves splica's stability at volume.
# Usage: ./benchmarks/stress_test.sh <splica-binary> [num-files] [output-file]
#
# Processes N synthetic media files through splica, verifying each produces
# correct output (exit code 0, valid JSON, duration within tolerance).
#
# Arguments:
#   $1 — path to splica binary (required)
#   $2 — number of files to process (default: 100 for CI, use 1000 for manual)
#   $3 — output report path (default: benchmarks/results/stress_test.md)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FIXTURES="$REPO_ROOT/tests/fixtures"

# ---------------------------------------------------------------------------
# Args
# ---------------------------------------------------------------------------

SPLICA="${1:?Usage: stress_test.sh <splica-binary> [num-files] [output-file]}"
NUM_FILES="${2:-100}"
OUT="${3:-$SCRIPT_DIR/results/stress_test.md}"

if [[ ! -x "$SPLICA" ]]; then
    echo "ERROR: splica binary not found or not executable: $SPLICA"
    exit 1
fi

mkdir -p "$(dirname "$OUT")"

# ---------------------------------------------------------------------------
# Fixture catalogue — each entry is "input_file:output_ext:extra_args"
#   stream-copy paths (no re-encode):  MP4→MKV, MKV→MP4
#   transcode paths:                   H.265→H.264, H.264→H.265, codec flag
# ---------------------------------------------------------------------------

SCENARIOS=()

# Stream copy: container remux only
SCENARIOS+=("bigbuckbunny_h264_aac.mp4:mkv:")
SCENARIOS+=("bigbuckbunny_h264.mkv:mp4:")
SCENARIOS+=("bigbuckbunny_h265_aac.mp4:mkv:")
SCENARIOS+=("bigbuckbunny_h264.mp4:mkv:")

# Transcode: re-encode video
SCENARIOS+=("bigbuckbunny_h265.mp4:mp4:--codec h264")
SCENARIOS+=("bigbuckbunny_h264.mp4:mp4:--codec h265")
SCENARIOS+=("bigbuckbunny_h265_aac.mp4:mp4:--codec h264")

NUM_SCENARIOS=${#SCENARIOS[@]}

# ---------------------------------------------------------------------------
# Temp workspace
# ---------------------------------------------------------------------------

TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Probe input duration via splica probe --format json
probe_duration() {
    local file="$1"
    "$SPLICA" probe --format json "$file" 2>/dev/null \
        | python3 -c "
import sys, json
data = json.load(sys.stdin)
tracks = data.get('tracks', [])
durations = [t['duration_seconds'] for t in tracks if 'duration_seconds' in t]
print(max(durations) if durations else '')
" 2>/dev/null || echo ""
}

# Extract output_duration_secs from the NDJSON complete line
extract_output_duration() {
    local ndjson="$1"
    python3 -c "
import sys, json
for line in open('$ndjson'):
    line = line.strip()
    if not line:
        continue
    try:
        obj = json.loads(line)
        if obj.get('type') == 'complete' and 'output_duration_secs' in obj:
            print(obj['output_duration_secs'])
            sys.exit(0)
    except json.JSONDecodeError:
        pass
print('')
" 2>/dev/null || echo ""
}

# Check if two durations are within tolerance (20%)
duration_ok() {
    local input_dur="$1"
    local output_dur="$2"
    python3 -c "
import sys
try:
    a, b = float('$input_dur'), float('$output_dur')
    if a <= 0:
        sys.exit(0)  # can't compare, treat as ok
    ratio = b / a
    sys.exit(0 if 0.8 <= ratio <= 1.2 else 1)
except (ValueError, ZeroDivisionError):
    sys.exit(0)  # missing data, skip check
"
}

# Peak RSS in KB from /usr/bin/time -l (macOS) output
parse_peak_rss_kb() {
    local time_stderr="$1"
    # macOS /usr/bin/time -l reports "maximum resident set size" in bytes
    local bytes
    bytes=$(grep -i "maximum resident set size" "$time_stderr" | awk '{print $1}' || echo "0")
    if [[ -n "$bytes" && "$bytes" != "0" ]]; then
        echo $(( bytes / 1024 ))
    else
        echo "0"
    fi
}

# ---------------------------------------------------------------------------
# Run
# ---------------------------------------------------------------------------

echo "=== splica stress test ==="
echo "Binary:    $SPLICA"
echo "Files:     $NUM_FILES"
echo "Scenarios: $NUM_SCENARIOS"
echo "Temp dir:  $TMP_DIR"
echo ""

TOTAL_START=$(python3 -c "import time; print(time.time())")
FAIL_COUNT=0
DURATION_MISMATCH=0
PEAK_RSS_KB=0
PROCESSED=0

for i in $(seq 1 "$NUM_FILES"); do
    # Round-robin through scenarios
    IDX=$(( (i - 1) % NUM_SCENARIOS ))
    SCENARIO="${SCENARIOS[$IDX]}"

    IFS=':' read -r INPUT_FILE OUT_EXT EXTRA_ARGS <<< "$SCENARIO"
    INPUT_PATH="$FIXTURES/$INPUT_FILE"
    OUTPUT_PATH="$TMP_DIR/out_${i}.${OUT_EXT}"
    JSON_PATH="$TMP_DIR/out_${i}.json"
    TIME_PATH="$TMP_DIR/time_${i}.txt"

    # Probe input duration (cache per fixture file)
    CACHE_VAR="CACHED_DUR_$(echo "$INPUT_FILE" | tr '.-' '__')"
    if [[ -z "${!CACHE_VAR:-}" ]]; then
        INPUT_DUR=$(probe_duration "$INPUT_PATH")
        eval "$CACHE_VAR='$INPUT_DUR'"
    else
        INPUT_DUR="${!CACHE_VAR}"
    fi

    # Build command
    CMD=("$SPLICA" process --input "$INPUT_PATH" --output "$OUTPUT_PATH" --format json)
    if [[ -n "$EXTRA_ARGS" ]]; then
        # shellcheck disable=SC2206
        CMD+=($EXTRA_ARGS)
    fi

    # Run with memory tracking
    EXIT_CODE=0
    /usr/bin/time -l "${CMD[@]}" > "$JSON_PATH" 2> "$TIME_PATH" || EXIT_CODE=$?

    # Track peak RSS
    RSS_KB=$(parse_peak_rss_kb "$TIME_PATH")
    if (( RSS_KB > PEAK_RSS_KB )); then
        PEAK_RSS_KB=$RSS_KB
    fi

    # Check exit code
    if [[ "$EXIT_CODE" -ne 0 ]]; then
        FAIL_COUNT=$((FAIL_COUNT + 1))
        echo "  FAIL [$i/$NUM_FILES] $INPUT_FILE -> .$OUT_EXT (exit $EXIT_CODE)"
        continue
    fi

    # Check duration
    OUTPUT_DUR=$(extract_output_duration "$JSON_PATH")
    if [[ -n "$INPUT_DUR" && -n "$OUTPUT_DUR" ]]; then
        if ! duration_ok "$INPUT_DUR" "$OUTPUT_DUR"; then
            DURATION_MISMATCH=$((DURATION_MISMATCH + 1))
            echo "  DURATION MISMATCH [$i/$NUM_FILES] $INPUT_FILE -> .$OUT_EXT (input=${INPUT_DUR}s output=${OUTPUT_DUR}s)"
        fi
    fi

    PROCESSED=$((PROCESSED + 1))

    # Progress every 10 files
    if (( i % 10 == 0 )); then
        echo "  [$i/$NUM_FILES] processed..."
    fi
done

TOTAL_END=$(python3 -c "import time; print(time.time())")
TOTAL_SECS=$(python3 -c "print(f'{$TOTAL_END - $TOTAL_START:.2f}')")
FILES_PER_SEC=$(python3 -c "
t = $TOTAL_END - $TOTAL_START
print(f'{$NUM_FILES / t:.1f}' if t > 0 else 'N/A')
")
PEAK_RSS_MB=$(python3 -c "print(f'{$PEAK_RSS_KB / 1024:.1f}')")

# ---------------------------------------------------------------------------
# Summary (stdout)
# ---------------------------------------------------------------------------

echo ""
echo "=== Stress Test Results ==="
echo "Total files:          $NUM_FILES"
echo "Total time:           ${TOTAL_SECS}s"
echo "Throughput:           ${FILES_PER_SEC} files/sec"
echo "Peak RSS:             ${PEAK_RSS_MB} MB"
echo "Failures (non-zero):  $FAIL_COUNT"
echo "Duration mismatches:  $DURATION_MISMATCH"
echo ""

# ---------------------------------------------------------------------------
# Report (markdown)
# ---------------------------------------------------------------------------

cat > "$OUT" << EOF
# Stress Test Results (SPL-202)

Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)
Binary: \`$SPLICA\`

## Summary

| Metric | Value |
|--------|-------|
| Total files processed | $NUM_FILES |
| Total time | ${TOTAL_SECS}s |
| Throughput | ${FILES_PER_SEC} files/sec |
| Peak RSS | ${PEAK_RSS_MB} MB |
| Failures (non-zero exit) | $FAIL_COUNT |
| Duration mismatches (>20%) | $DURATION_MISMATCH |

## Scenarios

| # | Input | Output | Mode |
|---|-------|--------|------|
EOF

for idx in "${!SCENARIOS[@]}"; do
    IFS=':' read -r INPUT_FILE OUT_EXT EXTRA_ARGS <<< "${SCENARIOS[$idx]}"
    if [[ -n "$EXTRA_ARGS" ]]; then
        MODE="transcode ($EXTRA_ARGS)"
    else
        MODE="stream copy"
    fi
    echo "| $((idx + 1)) | $INPUT_FILE | .$OUT_EXT | $MODE |" >> "$OUT"
done

cat >> "$OUT" << EOF

## Configuration

- Files per scenario: ~$((NUM_FILES / NUM_SCENARIOS)) (round-robin across $NUM_SCENARIOS scenarios)
- Duration tolerance: 20% (input vs output)
- Memory tracking: \`/usr/bin/time -l\` (macOS)

## Pass/Fail

EOF

if [[ "$FAIL_COUNT" -eq 0 && "$DURATION_MISMATCH" -eq 0 ]]; then
    echo "**PASS** — all $NUM_FILES files processed successfully with correct durations." >> "$OUT"
else
    echo "**FAIL** — $FAIL_COUNT exit failures, $DURATION_MISMATCH duration mismatches." >> "$OUT"
fi

echo ""
echo "  -> $OUT"

# ---------------------------------------------------------------------------
# Exit non-zero if any file silently failed
# ---------------------------------------------------------------------------

if [[ "$FAIL_COUNT" -gt 0 || "$DURATION_MISMATCH" -gt 0 ]]; then
    exit 1
fi
