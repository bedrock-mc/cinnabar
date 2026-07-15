# Phase 2 visibility/presentation authority correction

Date: 2026-07-15

## Scope

This correction makes the moving-camera FIFO/no-vsync A/B authoritative. It does
not change culling, cave connectivity, light solving, draw-mode selection, or the
wgpu dependency, and it does not claim that the visible artifact is fixed.

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
`Immediate`; both harnesses detect Bevy's pinned fallback message, record `Fifo`
as the effective mode when that happens, and reject that run as an invalid
no-vsync A/B.

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
- `cargo fmt --all -- --check`, PowerShell parser validation,
  `bash -n scripts/acceptance.sh`, and `git diff --check`: passed.
