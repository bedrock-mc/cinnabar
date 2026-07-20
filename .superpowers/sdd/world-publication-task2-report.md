# World publication performance Task 2 report

Date: 2026-07-15

Status: `DONE`

Branch: `phase2-world-publication-performance`

Base: `891c2bc` (`perf: skip unchanged light remeshes`)

Primary commit: `33c3e63` (`feat: attribute world publication latency`).

Review follow-up: this report is updated in the follow-up commit; its final
immutable hash is reported in the handoff because a commit cannot contain its
own hash.

## Scope

This commit implements Task 2 only from
`docs/superpowers/plans/2026-07-15-world-publication-performance.md`.
It does not modify `plan.md`, add or run the Task 3 benchmark/live gate, change
culling, cave connectivity, presentation mode, draw mode, light values, AO, or
shader output, and it adds no per-subchunk logging.

Task 1 light identities and outcome classification remain intact. Decode,
light, and mesh work now retain the `Instant` from their existing bounded
admission point. Worker start computes queue wait using
`saturating_duration_since`, and completions carry that value independently of
the existing worker duration. `WorldStreamStats` exposes cumulative maximum
queue waits for all three stages. The existing worker-duration update points
remain unchanged, including their stale-completion semantics.

Once per visibility-diagnostic interval, the client emits one coherent
`RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT` JSON row from one world-stream stats
copy, the matching render upload queue, the matching visibility generation and
draw mode, and the already-proven graphics/presentation metadata. The row
contains:

- accepted, no-op, value-changing, provenance-only, and mesh-invalidating light
  counters;
- stale light and mesh counters;
- decode queued/in-flight, light pending/in-flight, and mesh pending/in-flight
  gauges;
- decode/light/mesh maximum queue waits and worker durations as distinct
  millisecond fields;
- upload queue items, queued bytes, and cumulative GPU upload bytes;
- frame, pose, and view generations;
- exact draw mode, build profile, requested/effective present mode and proof;
- backend, adapter, driver, and driver-info provenance.

PowerShell and Bash consume the same exact flat schema. They reject absent or
extra fields, duplicate JSON keys, malformed JSON, invalid numeric ranges,
non-finite or negative durations, unproved or mismatched presentation, invalid
or changing draw modes, changing run identities, and any disagreement with the
one-shot runtime metadata. Therefore debug/release, Direct/MDI, and
FIFO/Immediate cannot be combined into one accepted result row.

## TDD RED evidence

Tests and script contracts were added before production changes.

1. Focused Rust stage contract:

   `cargo test -p bedrock-client publication_stage --locked -- --nocapture`

   RED: compilation failed on the intentionally missing
   `max_*_queue_wait` fields, fixed-instant saturating queue-wait helper, and
   stage queue observers. The same compile also failed on the missing coherent
   snapshot helper required by the main-module contract.

2. PowerShell script contract:

   `powershell -NoProfile -ExecutionPolicy Bypass -Command "Invoke-Pester 'scripts/tests/acceptance.Tests.ps1'"`

   RED: the script stopped at the intentionally missing
   `ConvertFrom-WorldPublicationSnapshotMarker` function.

3. Bash script contract:

   Git Bash was used because `C:\Windows\System32\bash.exe` is a WSL launcher
   and this host has no installed WSL distribution.

   `C:\Program Files\Git\bin\bash.exe scripts/tests/acceptance_test.sh`

   RED: the script stopped at the intentionally missing
   `read_world_publication_snapshots` function.

4. Worker-timer preservation correction:

   After self-review, the focused test was tightened to require queue-only
   observation so stale jobs could not broaden the existing worker maxima.
   RED: compilation failed on missing `observe_*_queue_wait` methods while the
   first implementation still exposed combined timing observers. The minimal
   correction separated queue observation and retained every original worker
   maximum update point.

## GREEN evidence

- `cargo test -p bedrock-client publication_stage --locked -- --nocapture`
  passed the fixed-instant queue/worker separation and nonshrinking maxima
  regression.
- `cargo test -p bedrock-client world_publication_snapshot --locked -- --nocapture`
  passed byte-deterministic snapshot construction, `u64::MAX` counter
  serialization, distinct queue/worker fields, uploads, draw, profile, and
  presentation provenance.
- The inherited Task 1 `light_outcome_counters_saturate` regression passed in
  the full client suite.
- `cargo test -p bedrock-client --locked` passed all client targets: 278 unit
  tests passed with the existing release-only benchmark ignored, followed by
  43, 14, and 14 passing integration-target tests.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1`
  passed and printed `acceptance.ps1 dry-run tests: PASS`.
- The required Pester wrapper was also verified through its result object:
  `Invoke-Pester scripts/tests/acceptance.Tests.ps1 -PassThru` reported
  `PESTER_FAILED_COUNT=0`.
- `C:\Program Files\Git\bin\bash.exe scripts/tests/acceptance_test.sh` passed
  and printed `acceptance.sh dry-run tests: PASS`. Because this Windows host's
  `python3.exe` entries are empty Microsoft Store aliases, the run used an
  isolated Python 3.12.8 embeddable interpreter under `%TEMP%`; no repository
  file or dependency was added.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed for
  the full workspace.
- `cargo fmt --all -- --check` and `git diff --check` passed.

## Correctness review

- Decode enqueue time is captured when each heavy event enters the bounded
  FIFO, including block updates admitted later by FIFO application.
- Light enqueue time is captured with the exact dirty revision timestamp and
  survives readiness deferral until worker start.
- Mesh enqueue time is separate from mutation/remesh `dirty_since`; stale mesh
  requeueing preserves end-to-end latency provenance while starting a new queue
  wait interval.
- Queue waits use saturating duration semantics and update only cumulative
  maxima. Timed sessions reset queue and worker high-water marks together while
  retaining cumulative counters.
- No completion emits a log line. One periodic marker joins one copied stats
  state with its render/upload and visibility/presentation identities.
- Shell schemas are exact and fail closed. Periodic rows may repeat, but draw,
  build, presentation, backend, adapter, and driver identities may not change
  within a run.
- Only the six Task 2 implementation/test files and this required report are
  changed. Task 1 storage identities and generation checks are preserved.

## Review follow-up

Two Important review findings were reproduced and fixed test-first without
changing the publication or rendering behavior.

1. PowerShell exact scalar types:

   RED: adversarial rows with `"17"`, `"41.0"`, and `"true"` were accepted
   because PowerShell casts JSON strings to decimal/double/Boolean-compatible
   comparison values. The parser now validates the flat JSON scalar token
   lexically before conversion: counters/gauges/generations require a JSON
   nonnegative integer token, durations require a JSON number token, identity
   fields require JSON strings, and present proof requires the literal JSON
   Boolean `true`. Matching Bash wrong-type regressions preserve exact-type
   parity.

2. Pester closure scope:

   RED: Windows PowerShell 5.1/Pester 3.4 ran the test-created readiness closure
   in a dynamic module that could not resolve the dot-sourced
   `Test-RakNetUnconnectedPong` function. The test now captures that exact
   helper scriptblock into the closure. Production readiness logic is
   unchanged. `Invoke-Pester ... -PassThru` now reports zero failures rather
   than relying on Pester 3.4's process exit code.

After the follow-up, the native PowerShell suite, Pester result-count gate,
Bash suite, full client suite, strict workspace Clippy, rustfmt, and diff check
all pass. No Task 2 correctness concern is known.
