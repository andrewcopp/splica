# Exit Code & Error Kind Contract v1.1

> **Stability notice:** This contract is stable. Changes require a semver bump.

This document defines the exit codes returned by the `splica` CLI binary. It is
intended for automation consumers who need to make retry/abort decisions based on
the process exit code.

## Exit Codes

| Code | Name                | Meaning                                                        | Retryable |
|------|---------------------|----------------------------------------------------------------|-----------|
| 0    | Success             | The operation completed successfully.                          | N/A       |
| 1    | Bad input           | Malformed file, unsupported format, or invalid arguments.      | No        |
| 2    | Internal error      | Encoder, muxer, or I/O failure.                                | Yes       |
| 3    | Resource exhausted  | Memory, file handles, or budget limits exceeded.               | Yes       |

### Retry guidance

- **Code 1** -- Do not retry. The input or arguments are invalid and the same
  invocation will always fail.
- **Code 2** -- May retry immediately. Failures in this category (e.g., I/O
  errors) are often transient.
- **Code 3** -- Retry after backoff. The system is under resource pressure and
  needs time to recover.

## JSON `error_kind` mapping

When running with `--format json`, error output includes an `error_kind` field.
The mapping from `error_kind` values to exit codes is:

| `error_kind`          | Exit Code | Retryable | `ErrorKind` variant    | Description                                                   |
|-----------------------|-----------|-----------|------------------------|---------------------------------------------------------------|
| `bad_input`           | 1         | No        | `InvalidInput`         | Malformed file, invalid arguments, or configuration error     |
| `unsupported_format`  | 1         | No        | `UnsupportedFormat`    | Recognized but unsupported codec or container format          |
| `internal_error`      | 2         | Yes       | `Io`, `Internal`       | Encoder/muxer/I/O failure (bug or transient)                  |
| `resource_exhausted`  | 3         | Yes       | `ResourceExhausted`    | Memory, file handles, or budget limits exceeded               |

These `error_kind` values use stable snake\_case strings that **will not change
across minor versions**. Adding a new value is a minor-version change; removing
or renaming an existing value is a breaking change.

Unrecognized errors (those not wrapping a typed splica library error) default to
`bad_input` with exit code 1. This covers CLI-level validation errors such as
invalid arguments.

A compile-time exhaustiveness test in `splica-cli` (`error_contract_tests.rs`)
ensures that adding a new `ErrorKind` variant without updating this contract
fails the build.

## JSON error output format

```json
{
  "type": "error",
  "error_kind": "bad_input",
  "message": "unsupported codec 'vp8' for container 'mp4'",
  "input": "bad_file.mp4"
}
```

The `input` field contains the verbatim input file path as passed by the caller.
It is `null` (or absent) when no input file was parsed before the error occurred
(e.g., flag parsing failures) or when the command operates on multiple inputs.

## `ErrorKind` retryability

The `ErrorKind` enum in `splica-core` exposes an `is_retryable()` method. The
retryable kinds are:

- `Io` -- transient I/O failures
- `ResourceExhausted` -- resource pressure

The non-retryable kinds are:

- `InvalidInput` -- malformed or invalid input
- `UnsupportedFormat` -- recognized but unsupported format/codec
- `Internal` -- bug in splica (should be reported)

## Version history

| Version | Date       | Changes                                                    |
|---------|------------|------------------------------------------------------------|
| v1.1    | 2026-03-16 | Added error_kind stability contract, retryability, and exhaustiveness test. |
| v1.0    | 2026-03-16 | Initial publication.                                       |
