#!/usr/bin/env bash
# Run all splica vs ffmpeg benchmark demos.
# Usage: ./benchmarks/run_all.sh [--quick]
#
# Requires: splica (release build), ffmpeg
# Produces: benchmarks/results/ with all output

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RESULTS_DIR="$SCRIPT_DIR/results"
SPLICA="$REPO_ROOT/target/release/splica"
FIXTURES="$REPO_ROOT/tests/fixtures"

# Check prerequisites
if [[ ! -x "$SPLICA" ]]; then
    echo "ERROR: Release binary not found. Run: cargo build --release"
    exit 1
fi
if ! command -v ffmpeg &>/dev/null; then
    echo "ERROR: ffmpeg not found in PATH"
    exit 1
fi

rm -rf "$RESULTS_DIR"
mkdir -p "$RESULTS_DIR"

echo "=== splica vs ffmpeg benchmark suite ==="
echo "Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "splica: $($SPLICA --version 2>&1 || echo 'dev')"
echo "ffmpeg: $(ffmpeg -version 2>&1 | head -1)"
echo ""

# Run each benchmark
for bench in "$SCRIPT_DIR"/bench_*.sh; do
    echo "--- Running $(basename "$bench") ---"
    bash "$bench" "$SPLICA" "$FIXTURES" "$RESULTS_DIR" "$@"
    echo ""
done

echo "=== Results written to $RESULTS_DIR ==="
echo ""

# Generate summary
bash "$SCRIPT_DIR/summary.sh" "$RESULTS_DIR"
