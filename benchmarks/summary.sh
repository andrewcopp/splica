#!/usr/bin/env bash
# Generate a combined summary of all benchmark results
set -euo pipefail

RESULTS="$1"
OUT="$RESULTS/SUMMARY.md"

cat > "$OUT" << 'HEADER'
# splica vs ffmpeg — Benchmark Summary

Generated on: DATEPLACEHOLDER

## What This Shows

These benchmarks compare splica and ffmpeg across five dimensions that matter
to real users. We are honest about where each tool wins.

### Where splica wins today

1. **CLI ergonomics** — fewer flags, sane defaults, named parameters
2. **Correct-by-default** — resize letterboxes, stream copy auto-detects
3. **Structured errors** — typed exit codes + JSON events for automation
4. **Migration path** — `splica migrate` translates ffmpeg commands
5. **WASM bundle** — browser-native demuxing without shipping 30 MB

### Where ffmpeg wins today

1. **Raw encode throughput** — decades of SIMD optimization
2. **Format coverage** — hundreds of codecs and containers
3. **Ecosystem maturity** — 24 years of production hardening
4. **Hardware acceleration** — NVENC, QSV, VideoToolbox

### The thesis

splica is not trying to replace ffmpeg for every use case. It targets the
90% of production workloads (H.264/H.265/AV1 + AAC/Opus in MP4/WebM/MKV)
where the right defaults and a clean API matter more than supporting every
obscure codec ever invented.

## Detailed Results

HEADER

# Replace date placeholder
sed -i '' "s/DATEPLACEHOLDER/$(date -u +%Y-%m-%dT%H:%M:%SZ)/" "$OUT"

# Append all result files
for f in "$RESULTS"/ergonomics.md "$RESULTS"/performance.md "$RESULTS"/error_handling.md "$RESULTS"/aspect_ratio.md "$RESULTS"/wasm_bundle.md; do
    if [[ -f "$f" ]]; then
        echo "---" >> "$OUT"
        echo "" >> "$OUT"
        cat "$f" >> "$OUT"
        echo "" >> "$OUT"
    fi
done

echo "  -> $OUT"
echo ""
echo "=== SUMMARY ==="
cat "$OUT"
