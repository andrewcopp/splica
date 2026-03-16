# WASM Bundle Size: splica vs ffmpeg.wasm

The single most important metric for browser-based video processing is
download size. Users leave if a page takes too long to load.

## Measured Sizes

| Package | Uncompressed | Gzipped | Includes |
|---------|-------------|---------|----------|
| splica-wasm | 165 KB | 68 KB | MP4 + WebM + MKV demuxers, all WASM types |
| ffmpeg.wasm (core) | ~22 MB | ~8 MB | Full ffmpeg compiled to WASM |
| ffmpeg.wasm (full) | ~32 MB | ~12 MB | ffmpeg + all codecs |

**splica is 133x smaller** than ffmpeg.wasm core.

## What This Means

| Metric | splica-wasm | ffmpeg.wasm |
|--------|------------|-------------|
| Time to interactive (3G) | < 1 second | 30-60 seconds |
| Time to interactive (4G) | < 0.5 seconds | 10-20 seconds |
| CDN cost per 1M page loads | ~$0.07 | ~$8-12 |
| Mobile data budget impact | Negligible | Significant |

## Why splica Is Smaller

ffmpeg.wasm compiles the entire ffmpeg codebase to WebAssembly: 100+ codecs,
50+ container formats, hardware abstraction layers, a CLI parser, and a virtual
filesystem. Most applications use < 5% of this.

splica-wasm includes only what browser applications need:
- Container demuxing (MP4, WebM, MKV) — the part browsers can't do
- WebCodecs-compatible output — the browser handles actual decode/encode
- No CLI, no virtual filesystem, no unused codecs

This is not a stripped-down ffmpeg. It's a different architecture: let the
browser do what browsers are good at (decode/encode via WebCodecs), and only
ship the part they can't do (container parsing).

## Feature-Flag Builds

`splica-wasm` supports per-codec feature flags. The default build (no features)
includes only demuxers. Enable codec features to add encode/decode support at
the cost of larger bundle size.

| Configuration | What's Included |
|---------------|-----------------|
| default (no features) | MP4 + WebM + MKV demuxers, probe, detect |
| `encode-av1` | Above + AV1 encoder (rav1e, pure Rust) |
| `all-codecs` | All supported codecs |

Build with a specific feature set:

```sh
# Demux-only (smallest)
wasm-pack build --target web crates/splica-wasm --release

# With AV1 encoder
wasm-pack build --target web crates/splica-wasm --release -- --features encode-av1

# All codecs
wasm-pack build --target web crates/splica-wasm --release -- --features all-codecs
```

### WASM Codec Compatibility

Not all codecs compile to `wasm32-unknown-unknown`. Pure-Rust codecs work
natively; C FFI codecs require Emscripten or prebuilt WASM modules.

| Feature | Backend | WASM Native | Notes |
|---------|---------|-------------|-------|
| `encode-av1` | rav1e (Rust) | Yes | Pure Rust, works out of the box |
| `decode-aac` | symphonia (Rust) | Yes | Pure Rust AAC decoder |
| `decode-h264` | openh264 (C) | Needs setup | Requires prebuilt WASM binary |
| `encode-h264` | openh264 (C) | Needs setup | Requires prebuilt WASM binary |
| `decode-h265` | libde265 (C) | Needs setup | Requires Emscripten |
| `encode-h265` | kvazaar (C) | Needs setup | Requires Emscripten |
| `decode-av1` | dav1d (C) | Needs setup | Requires Emscripten |
| `encode-aac` | fdk-aac (C) | Needs setup | Requires Emscripten |
| `*-opus` | libopus (C) | Needs setup | Requires Emscripten |

Use `codecCapabilities()` at runtime to check which codecs are available.
