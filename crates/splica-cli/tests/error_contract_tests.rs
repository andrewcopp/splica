//! Error contract exhaustiveness tests.
//!
//! Ensures the documented set of `error_kind` strings stays in sync with the
//! code. If you add a new `ErrorKind` variant, these tests will fail until
//! you update the contract in `docs/exit-codes.md` and the lists below.

use splica_core::ErrorKind;

/// The set of `error_kind` strings documented in `docs/exit-codes.md`.
///
/// If a test below fails, you added a new `ErrorKind` variant — update
/// `docs/exit-codes.md` and this list.
const DOCUMENTED_ERROR_KINDS: &[&str] = &[
    "bad_input",
    "internal_error",
    "resource_exhausted",
    "unsupported_format",
];

#[test]
fn test_that_all_error_kind_strings_are_documented() {
    let mut actual: Vec<&str> = ErrorKind::ALL
        .iter()
        .map(|k| k.as_error_kind_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    actual.sort();

    let mut expected: Vec<&str> = DOCUMENTED_ERROR_KINDS.to_vec();
    expected.sort();

    assert_eq!(
        actual, expected,
        "error_kind strings in code do not match documented set. \
         If you added a new ErrorKind variant, update docs/exit-codes.md \
         and DOCUMENTED_ERROR_KINDS in this test."
    );
}

#[test]
fn test_that_all_error_kinds_have_valid_exit_codes() {
    for &kind in ErrorKind::ALL {
        let code = kind.exit_code();

        assert!(
            (1..=3).contains(&code),
            "ErrorKind::{kind:?} has unexpected exit code {code}"
        );
    }
}

#[test]
fn test_that_error_kind_strings_are_snake_case() {
    for &kind in ErrorKind::ALL {
        let s = kind.as_error_kind_str();

        assert_eq!(
            s,
            s.to_lowercase(),
            "error_kind string {s:?} is not lowercase"
        );
        assert!(
            !s.contains(' ') && !s.contains('-'),
            "error_kind string {s:?} is not snake_case"
        );
    }
}

#[test]
fn test_that_exit_code_matches_retryability() {
    for &kind in ErrorKind::ALL {
        let code = kind.exit_code();

        if code == 1 {
            assert!(
                !kind.is_retryable(),
                "ErrorKind::{kind:?} has exit code 1 (bad input) but is marked retryable"
            );
        }
    }
}
