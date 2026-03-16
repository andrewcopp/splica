# Exit Code Contract v1.0

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

| `error_kind`          | Exit Code | `ErrorKind` variant    |
|-----------------------|-----------|------------------------|
| `bad_input`           | 1         | `InvalidInput`         |
| `unsupported_format`  | 1         | `UnsupportedFormat`    |
| `internal_error`      | 2         | `Io`, `Internal`       |
| `resource_exhausted`  | 3         | `ResourceExhausted`    |

Unrecognized errors (those not wrapping a typed splica library error) default to
`bad_input` with exit code 1. This covers CLI-level validation errors such as
invalid arguments.

## JSON error output format

```json
{
  "type": "error",
  "error_kind": "bad_input",
  "message": "unsupported codec 'vp8' for container 'mp4'"
}
```

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

| Version | Date       | Changes              |
|---------|------------|----------------------|
| v1.0    | 2026-03-16 | Initial publication. |
