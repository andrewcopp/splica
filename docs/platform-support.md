# Platform Support

## iOS (`aarch64-apple-ios`)

Verified: 2026-03-16 (SPL-204)

### Pure-Rust Crates

All six pure-Rust crates compile for `aarch64-apple-ios` with no code changes required.

| Crate            | Compiles | Notes                        |
|------------------|----------|------------------------------|
| splica-core      | Yes      | No issues                    |
| splica-mp4       | Yes      | No issues                    |
| splica-webm      | Yes      | No issues                    |
| splica-mkv       | Yes      | No issues                    |
| splica-filter    | Yes      | No issues                    |
| splica-pipeline  | Yes      | No issues                    |

### Not Tested

| Crate        | Reason                                              |
|--------------|-----------------------------------------------------|
| splica-codec | FFI dependencies (openh264, dav1d, libopus) require cross-compilation toolchains |
| splica-cli   | Binary target, not relevant for library embedding   |

### Summary

The pure-Rust layer of splica is fully compatible with iOS. An iOS app could
depend on `splica-pipeline` (and everything below it) today for demuxing,
muxing, and filter-graph work without any platform-specific patches.

Codec support (`splica-codec`) would require cross-compiling the native C
libraries (openh264, dav1d, kvazaar, libopus, fdk-aac) for the iOS target,
which is a separate effort.
