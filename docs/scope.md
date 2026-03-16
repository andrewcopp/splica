# Scope: What splica Is and Is Not

## What splica is

splica is a file-to-file media processing library and CLI. It targets the 90% of
production video/audio workloads: the codecs, containers, and operations that
modern services actually use.

### Supported codecs

| Type  | Codecs              |
|-------|---------------------|
| Video | H.264, H.265, AV1   |
| Audio | AAC, Opus            |

### Supported containers

MP4 (ISO BMFF), WebM, MKV.

### Supported operations

- **Transcode** -- re-encode video and/or audio between supported codecs
- **Stream copy** -- remux without re-encoding (fast, lossless)
- **Trim** -- extract a time range from a file
- **Join** -- concatenate multiple files into one
- **Scale** -- resize video to a target resolution
- **Crop** -- remove pixels from video edges
- **Volume** -- adjust audio level
- **Probe** -- inspect container metadata, tracks, and codec parameters
- **Extract audio** -- pull the audio track out of a video file

## What splica is not

splica is not a general-purpose media framework. It does not handle:

- **Live streaming** -- no RTMP, SRT, HLS ingest, or any protocol-based I/O.
  splica reads files and writes files.
- **Real-time processing** -- no frame-by-frame callbacks, no capture device
  integration, no low-latency pipelines.
- **Unbounded streams** -- input must have a known duration. Pipe-based infinite
  streams are not supported.
- **Hardware acceleration** -- no NVENC, QSV, VideoToolbox, or VA-API. All
  encoding runs in software.

## When to use ffmpeg instead

ffmpeg is the right tool when you need:

- **Live/RTMP/SRT ingest** -- splica has no network protocol support.
- **Exotic codecs** -- ProRes, DNxHR, VP8, Theora, FLAC, or anything outside
  the H.264/H.265/AV1 + AAC/Opus set.
- **Hardware encode** -- NVENC, QSV, VideoToolbox, VA-API. If throughput per
  watt matters more than memory safety, use ffmpeg with hardware backends.
- **Complex filter graphs** -- multi-input overlays, picture-in-picture,
  drawtext, audio mixing beyond simple volume adjustment.
- **Niche containers** -- FLV, MPEG-TS muxing, raw bitstream output, or
  formats splica does not support.

## Trade-offs

splica prioritizes safety and ergonomics over raw throughput. Concretely:

- **Software-only encoding** means splica will be slower than hardware-
  accelerated ffmpeg on the same machine. For batch workloads where cost is
  measured in CPU-hours, this is usually acceptable. For real-time encoding at
  scale, it is not.
- **Smaller codec surface** means fewer things can go wrong, but also fewer
  things you can do. If your pipeline touches a codec splica does not support,
  you need ffmpeg (or both).
- **Rust's safety guarantees** eliminate entire classes of CVEs (buffer
  overflows, use-after-free, data races) that are endemic to C-based media
  libraries. For services processing untrusted user uploads, this is the point.
