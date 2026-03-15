---
name: Monitor CI after pushing
description: Always verify CI passes after pushing changes, especially when modifying build dependencies or FFI code
type: feedback
---

CI was broken for 34 consecutive commits without anyone noticing. After pushing changes, monitor the CI run to confirm it passes.

**Why:** FFI codec dependencies (libde265, dav1d, kvazaar) require system libraries that aren't pre-installed on CI runners. Tests pass locally on macOS but fail on ubuntu-latest without apt-get install.

**How to apply:** When pushing changes that touch CI config, FFI code, or build dependencies, watch the CI run. Don't assume local success means CI will pass.
