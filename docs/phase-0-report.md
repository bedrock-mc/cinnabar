# Phase 0 acceptance report

## Decision

**CONDITIONAL GO — all Windows gates pass.** The deterministic protocol, core shutdown, app instrumentation, dry-run automation, and full 900-second Windows renderer/remesh session are green. Final Phase 0 GO remains pending only on the authoritative dev MacBook `p99_frame_ms <= 8` run.

This report is intentionally evidence-first: a gate remains pending until the corresponding run artifact exists under `.local/acceptance/<timestamp>/`.

## Pinned protocol repairs

| Area | Evidence | Result |
| --- | --- | --- |
| `AvailableCommands` shared enum count | Pinned-gophertunnel fixture, 165 bytes, SHA-256 `3d6e1870c49d643fe3f3b901cbbba40f49768cfe408c8b6ee136b5304ac1c98f` | Pass |
| `AvailableCommands` recorded-size regression | Structurally generated pinned-gophertunnel fixture, 356,519 batch bytes / 356,513 packet-body bytes, SHA-256 `08dea656b782928828fa79fc004166220f14d0459e259717bb537c5a11f6b39a` | Pass |
| `MaterialReducer` output vector | Pinned-gophertunnel fixture, 18 bytes, SHA-256 `b73c651ccf07ece21aea4b186be3780875ce7cacef04f9327e3c968636d43a39` | Pass |
| Owned decode, borrowed materialization, exact re-encode, malformed/oversized/truncated rejection | `cargo test -p protocol --test available_commands --test material_reducer -- --nocapture` | Pass: 11 tests |
| Login continues through `AvailableCommands` | Guarded test plus the Windows acceptance artifact below | Pass |

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
| Full Windows session | `session_seconds >= 900` from a Windows live artifact | Pass: 900.0015 s |
| Radius 16 received and rendered | requested, received, and publisher radii all equal 16; nonzero rendered/resident/visible counts | Pass: 16/16/16; 7,866/9,132/7,866 |
| Decode correctness | `decode_error_count == 0` | Pass: 0 |
| Visible live remesh | `visible_mutation_count > 0` and `max_mutation_to_visible_ms <= 100` | Pass: 432 mutations; 45.4522 ms max |
| Windows uncapped performance | p50/p95/p99/max frame times recorded | Pass: 3.0/4.1/5.1/36.8944 ms |
| Bounded pipeline | Peak queue/decode/mesh depths and GPU upload bytes recorded | Pass: recorded in canonical artifact |
| Dev MacBook performance | Authoritative `p99_frame_ms <= 8` on the specified machine | Pending |

## Windows live result

The passing run is preserved at `.local/acceptance/20260711T012409Z-48744/`; `.local/acceptance/windows-metrics.json` is its canonical metrics copy. Both paths are ignored and contain no committed Mojang assets.

| Measurement | Result |
| --- | ---: |
| Repository commit | `389853051a3b6d9826f6265c19754a09c435e655` |
| Session | 900.0015 s, 290,444 frames |
| Frame p50 / p95 / p99 / max | 3.0 / 4.1 / 5.1 / 36.8944 ms |
| Visible mutations | 432 |
| Max mutation to GPU-visible | 45.4522 ms |
| Decode errors | 0 |
| Max decode / mesh | 1.5181 / 5.5442 ms |
| Rendered / resident / visible subchunks | 7,866 / 9,132 / 7,866 |
| Peak admitted / heavy world events | 35 / 32 |
| Peak queued / in-flight decode jobs | 7 / 4 |
| Peak pending / in-flight mesh jobs | 5,576 / 106 |
| GPU upload bytes | 28,050,272 |

The app reached readiness only after the exact mutation target was rendered, cave-visible, GPU-acknowledged, all bounded pipeline work drained, and the state stayed unchanged for two seconds. It then auto-flew even while unfocused, alternated all mutations through owned BDS stdin, wrote the metrics, and shut down BDS/core/client cleanly. Windows p99 is supporting evidence; the plan's threshold decision remains assigned to the dev MacBook.

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

The passing Windows artifact was produced by the documented command. The harness validated every Windows gate, copied the canonical metrics, marked metadata `status: passed`, and left no BDS, core, or client process running.
