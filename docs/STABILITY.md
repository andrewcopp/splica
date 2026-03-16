# Stability Policy

## Current version: v0.x (pre-1.0)

splica is pre-1.0. The API, CLI surface, and output formats may change between
minor versions. Pin to an exact version if you need stability now.

## Post-1.0 commitments

Once splica reaches 1.0, the following stability guarantees apply:

### CLI flag surface -- stable

CLI flags are part of the public API. Removing a flag or changing its meaning is
a breaking change that requires a major version bump. Adding new flags is not
breaking.

### JSON output schema -- additive-only

Fields in JSON output (e.g., `splica probe --format json`) will not be removed
or have their types changed. New fields may be added at any time. Consumers
should ignore unknown fields.

### Exit code contract -- stable

Exit codes and their meanings are defined in [docs/exit-codes.md](exit-codes.md)
and are stable. Changing the meaning of an exit code or reassigning a code to a
different error category is a breaking change.

### Probe output fields -- additive-only

Fields returned by `splica probe` will not be removed. New fields may be added.
This applies to both human-readable and JSON output.

### Library API -- standard Rust semver

The Rust library crates follow standard semver. Public types, traits, and
functions are the API surface. `#[doc(hidden)]` items and items behind
`__unstable` feature flags are excluded from stability guarantees.

## What is NOT guaranteed

### Bitstream reproducibility

Encoder output is not bitstream-reproducible across splica versions. Upgrading
the underlying encoder library (openh264, rav1e, fdk-aac, libopus) may produce
different output bytes for the same input and settings. The output will be
semantically equivalent (same resolution, duration, codec parameters) but not
byte-identical.

**This is not a breaking change.** If your workflow requires bitstream
reproducibility, pin the exact splica version and do not upgrade.

## What IS a breaking change

The following changes require a major version bump after 1.0:

- Removing a CLI flag or subcommand
- Changing the meaning of an exit code
- Removing a field from JSON output
- Changing the type of an existing JSON field
- Changing `error_kind` values in JSON error output
- Removing a public type, trait, or function from the library API
- Changing the signature of a public library function in a non-backward-
  compatible way
