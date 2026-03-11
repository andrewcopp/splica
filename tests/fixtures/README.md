# Test Fixtures

Real media files used for integration testing. Each file is a 10-second,
640x360 clip from the Big Buck Bunny short film.

## Files

| File | Codec | Container | Size | Source |
|------|-------|-----------|------|--------|
| `bigbuckbunny_h264.mp4` | H.264 | MP4 | ~1 MB | test-videos.co.uk |
| `bigbuckbunny_h265.mp4` | H.265/HEVC | MP4 | ~1 MB | test-videos.co.uk |
| `bigbuckbunny_vp9.webm` | VP9 | WebM | ~1 MB | test-videos.co.uk |
| `bigbuckbunny_av1.mp4` | AV1 | MP4 | ~480 KB | Transcoded from H.264 fixture |

## Provenance

All files are sourced from [test-videos.co.uk](https://test-videos.co.uk),
which provides test clips from the Blender Foundation's
[Big Buck Bunny](https://peach.blender.org/) short film.

Big Buck Bunny is released under the
[Creative Commons Attribution 3.0](https://creativecommons.org/licenses/by/3.0/)
license by the Blender Foundation.

## Reproduction

The AV1 fixture was generated from the H.264 source:

```bash
ffmpeg -i bigbuckbunny_h264.mp4 -c:v libsvtav1 -crf 40 -preset 8 -an -y bigbuckbunny_av1.mp4
```

## Notes

- All files are video-only (no audio tracks). Audio fixture files
  (H.264+AAC) will be added in a future update.
- These files are small enough (~1 MB each) for regular git storage.
  Git LFS is not required.
