# Phase 0 acceptance report

## Decision

**PENDING — no Phase 0 GO claim.** The deterministic protocol, core shutdown, app instrumentation, and acceptance dry-run evidence are green. The required 900-second Windows renderer/remesh session has not been run, so none of its runtime gates are claimed. The authoritative dev MacBook `p99 <= 8 ms` gate is also pending.

This report is intentionally evidence-first: a gate remains pending until the corresponding run artifact exists under `.local/acceptance/<timestamp>/`.

## Pinned protocol repairs

| Area | Evidence | Result |
| --- | --- | --- |
| `AvailableCommands` shared enum count | Pinned-gophertunnel fixture, 165 bytes, SHA-256 `3d6e1870c49d643fe3f3b901cbbba40f49768cfe408c8b6ee136b5304ac1c98f` | Pass |
| `AvailableCommands` recorded-size regression | Structurally generated pinned-gophertunnel fixture, 356,519 batch bytes / 356,513 packet-body bytes, SHA-256 `08dea656b782928828fa79fc004166220f14d0459e259717bb537c5a11f6b39a` | Pass |
| `MaterialReducer` output vector | Pinned-gophertunnel fixture, 18 bytes, SHA-256 `b73c651ccf07ece21aea4b186be3780875ce7cacef04f9327e3c968636d43a39` | Pass |
| Owned decode, borrowed materialization, exact re-encode, malformed/oversized/truncated rejection | `cargo test -p protocol --test available_commands --test material_reducer -- --nocapture` | Pass: 11 tests |
| Login continues through `AvailableCommands` | Guarded `crates/protocol/tests/login.rs` test | Compiles and skips without `BEDROCK_BDS_DIR`; live evidence pending |

The exact upstream evidence and deviations are recorded in `crates/protocol/DEVIATIONS.md` and `crates/protocol/UPSTREAM.md`.

## Acceptance automation

Windows:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/acceptance.ps1 `
  -DurationSeconds 900 `
  -BdsDir .local/bds/bedrock-server-1.26.32.2 `
  -MetricsOut .local/acceptance/windows-metrics.json
```

macOS/Linux with a local BDS:

```bash
scripts/acceptance.sh \
  --duration 900 \
  --bds-dir .local/bds/bedrock-server-1.26.32.2 \
  --metrics-out .local/acceptance/mac-metrics.json
```

The shell harness also accepts `--upstream host:port`. It waits for a valid RakNet unconnected pong before starting core. A live external-upstream run must additionally pass `--mutation-command <executable>`; the harness gives that executable each complete `setblock` console command as one argument, allowing an SSH or host-specific wrapper to relay the deterministic mutation to the upstream BDS console. Dry-run command resolution does not require the relay.

Each live run writes its command/commit manifest before mutable preparation or builds, builds exact release binaries, prints and records the resolved BDS/core/app commands, waits for BDS/core/world readiness in order, and only then starts the timed 900-second metrics window. It alternates gold/diamond blocks at the app-emitted absolute coordinate through owned BDS stdin or the external relay, validates the metrics schema and gates, and streams stdout/stderr directly to artifact files under `.local/acceptance/<timestamp>-<pid>/`. Graceful shutdown is bounded and reaped before any forced-termination fallback completes.

Dry-run suites:

```text
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1
acceptance.ps1 dry-run tests: PASS

C:\Program Files\Git\bin\bash.exe scripts/tests/acceptance_test.sh
acceptance.sh dry-run tests: PASS
```

The checked-in verification command omits `-ExecutionPolicy Bypass`; this machine's execution policy blocks all `.ps1` files. Adding process-scoped `-ExecutionPolicy Bypass` executes the same dry-run and makes no persistent policy change.

## Gate status

| Gate | Required evidence | Status |
| --- | --- | --- |
| Protocol 1001 deviations resolved | Focused deterministic fixtures/tests | Pass |
| Graceful Windows core stop | EOF cancellation unit tests | Pass |
| Harness input validation / command resolution / no-launch dry run | PowerShell and Bash dry-run tests | Pass |
| Full Windows session | `session_seconds >= 900` from a Windows live artifact | Pending |
| Radius 16 received and rendered | requested, received, and publisher radii all equal 16; nonzero rendered/resident/visible counts | Pending |
| Decode correctness | `decode_error_count == 0` | Pending |
| Visible live remesh | `visible_mutation_count > 0` and `max_mutation_to_visible_ms <= 100` | Pending |
| Windows uncapped performance | p50/p95/p99/max frame times recorded | Pending |
| Bounded pipeline | Peak queue/decode/mesh depths and GPU upload bytes recorded | Pending |
| Dev MacBook performance | Authoritative `p99_frame_ms <= 8` on the specified machine | Pending |

## Expected live artifact layout

```text
.local/acceptance/<timestamp>-<pid>/
  metadata.json
  commands.txt
  build-app.log
  build-core.log
  bds.stdout.log
  bds.stderr.log
  bds.console.log
  core.stdout.log
  core.stderr.log
  app.stdout.log
  app.stderr.log
  app-metrics.json
  validated-metrics.json       # Windows harness after all gates pass
```

No 900-second acceptance run was launched while preparing this report. Consequently, there is no live artifact path to cite yet and the gate table must remain pending.
