# Phase 2 visibility/presentation authority correction

Date: 2026-07-15

## Scope

This correction makes the moving-camera FIFO/no-vsync A/B authoritative. It does
not change culling, cave connectivity, light solving, draw-mode selection, or the
selected wgpu version/features. The render crate now names the already pinned wgpu
API directly so it can query surface capabilities. This report does not claim that
the visible artifact is fixed.

The diagnostic path now retains bounded exact key sets only when diagnostics are
enabled. It reports deterministic missing and extra identities across resident,
cave-visible, frustum-visible, submitted, and GPU-completed stages. A submitted
frame is published only after the existing queue work-done sentinel completes, so
all stages in one marker share the same camera and view generation. Direct and MDI
submission paths use the same set semantics. Overflow invalidates exact evidence
instead of publishing a truncated digest as authoritative.

The acceptance harness now builds and validates a release client, requests FIFO by
default, and exposes no-vsync only through the explicit `-NoVsync`/`--no-vsync`
A/B option. Runtime metadata records the compiled profile, requested/effective
presentation policy, backend, adapter, driver, and driver information. Blank
adapter fields are recorded as `unavailable`. The explicit no-vsync path requests
`Immediate`. A render-subapp proof queries a surface created from the primary
extracted window's raw handle with the same `RenderInstance` and `RenderAdapter`,
then applies Bevy's pinned explicit-mode fallback order to those capabilities.
The structured marker distinguishes requested and effective modes and carries
`present_mode_proven=true`; both harnesses reject missing/false proof and reject
effective FIFO as an invalid no-vsync A/B. They do not depend on Bevy INFO logs.

## RED evidence

1. Exact moving-set regression:

   `cargo test -p render disjoint_moving_sets_cannot_report_zero_missing_keys --locked`

   Failed with `left: 0, right: 1` for the missing count when `{a}` moved to
   `{b,c}`. This reproduced the saturating-count/wrapping-hash defect.

2. Acceptance default:

   `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1`

   Failed at `default dry-run output changed` after the test required release/FIFO
   metadata and removal of the implicit `--no-vsync` flag.

3. Rust presentation contract:

   `cargo test -p bedrock-client acceptance_present_mode_is_fifo_unless_no_vsync_is_explicit --locked`

   First failed to compile because the requested-mode seam did not exist. A later
   RED run observed `AutoNoVsync` where the tightened contract required
   `Immediate`. The final test proves `Fifo` by default and `Immediate` only
   explicitly.

4. Runtime provenance:

   Focused render/client tests initially failed to compile because adapter metadata
   publication and the runtime metadata marker did not exist. A separate focused
   regression also failed before blank adapter fields had a bounded `unavailable`
   representation.

## GREEN evidence

- `cargo test -p render visibility_diagnostics --locked`: 11 focused tests passed.
- `cargo test -p bedrock-client runtime_metadata --locked`: focused runtime
  metadata test passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1`:
  passed.
- `C:\Program Files\Git\bin\bash.exe scripts/tests/acceptance_test.sh`: passed.
- `cargo test -p render --locked`: passed (151 unit tests plus all render
  integration and doc-test targets).
- `cargo test -p bedrock-client --locked`: passed (263 unit tests, one existing
  release-only test ignored, plus 35/14/14 integration tests).
- `cargo clippy -p render -p bedrock-client --all-targets --locked -- -D warnings`:
  passed.
- `cargo check --release -p bedrock-client --locked`: passed, proving the
  production dependency/features path builds without the test-only noop feature.
- `cargo fmt --all -- --check`, PowerShell parser validation,
  `bash -n scripts/acceptance.sh scripts/tests/acceptance_test.sh`, and
  `git diff --check`: passed.
- `cargo fmt --all -- --check`, PowerShell parser validation,
  `bash -n scripts/acceptance.sh`, and `git diff --check`: passed.

## Review correction: authoritative effective present mode

The first implementation copied the requested mode into the runtime marker and
inferred fallback from a suppressible Bevy INFO message. With `RUST_LOG=warn`, an
unsupported `Immediate` request could therefore be accepted incorrectly.

### Review RED evidence

- `cargo test -p render explicit_present_mode_evidence_comes_from_surface_capabilities --locked`
  failed with four `E0425` errors because the capability resolver did not exist.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1`
  failed with `runtime metadata without present-mode proof was accepted`.
- `C:\Program Files\Git\bin\bash.exe scripts/tests/acceptance_test.sh` exited 1
  while the harness still contained the suppressible fallback-log dependency and
  relabeled dry-run requests as effective modes.
- `cargo test -p bedrock-client runtime_metadata_marker_records_build_presentation_and_adapter_identity --locked`
  failed with `E0063`/`E0560` because presentation evidence was still sourced from
  `AcceptanceRuntimeConfig`, not renderer-published capability proof.

### Review GREEN evidence

- `cargo test -p render explicit_present_mode_evidence_comes_from_surface_capabilities --locked`:
  1 focused test passed.
- `cargo test -p bedrock-client runtime_metadata_marker_records_build_presentation_and_adapter_identity --locked`:
  1 focused test passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1`:
  passed with `acceptance.ps1 dry-run tests: PASS`.
- `C:\Program Files\Git\bin\bash.exe scripts/tests/acceptance_test.sh`: passed with
  `acceptance.sh dry-run tests: PASS`.
- `cargo test -p render --locked`: passed (152 unit tests and all integration/doc
  targets).
- `cargo test -p bedrock-client --locked`: passed (263 unit tests, one existing
  release-only test ignored, plus 35/14/14 integration tests).
- `cargo clippy -p render -p bedrock-client --all-targets --locked -- -D warnings`:
  passed.
