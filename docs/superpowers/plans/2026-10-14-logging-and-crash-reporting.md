# Logging, Tracing and Crash Reporting Plan

**Goal:** Give every Cinnabar process complete, bounded, local diagnostics: early and lossless
logs with category-level control, an Unreal-style crash monitor that writes a minidump and a
report folder and tells the user what happened, Go core crash capture, an always-on packet
flight recorder with opt-in full capture, and published symbols for symbolication. Nothing is
uploaded; sharing is always an explicit user action.

**Stack:** Rust 1.99, Bevy 0.20, `tracing` 0.1, `tracing-subscriber` 0.3, Embark
`crash-handler`/`minidumper`/`minidump-writer`, rust-minidump `minidump-processor`, Go 1.26
`log/slog` and `runtime/debug`.

## Decisions

| Question | Decision |
|---|---|
| Crash dialog | A crash window drawn with the pre-game OreUI stack (`bootstrap-ui`: theme, open font, panorama, logo): summary card, scrollable details, Restart, Copy report, Open folder, Close and Join our Discord. When no window can open, a native dialog (`launcher_host::native_dialog`) offers the same actions. |
| Crash monitor | Out of process. The client re-executes itself as `cinnabar --crash-monitor`, following the `--first-run-setup` precedent, so packaging gains no binary. |
| Minidump contents | Thread stacks, thread list and module list. `CINNABAR_FULL_DUMP=1` adds full memory for developers. |
| Symbols | Release builds keep line tables in split debug files; shipped binaries stay stripped; symbol archives are GitHub release assets. |
| Packet logging | Always-on header ring and decode-failure samples; full capture is opt-in per session in the `bedrock-replay` format. |
| Redaction | Typed secrets never format at write time; pattern scrubbing runs at share time from one rule file. |

## Current state

- `app/src/app/logging.rs` configures Bevy's `LogPlugin`, which installs `LogTracer`, the
  global subscriber, the Tracy `tracy.frame_mark` filter and the Android logcat layer
  (`bevy_log-0.20.0/src/lib.rs:305-418`). It runs at `app/src/app.rs:719`; `tracing` events
  from `lifecycle::before_run` and early `run()` are lost.
- `crates/diagnostics/src/console.rs` queues 4,096 writes and drops on overflow, including
  the file copy. `client.log` keeps one `.1` generation; `first-run.log` is unbounded.
  Terminal launches never copy raw `eprintln!` or the default panic output into the file.
- `crates/launcher-host/src/lifecycle/crash.rs` writes panic JSON whose `log_tail` reads
  `core.log` (`crash.rs:41`) instead of `client.log`. Windows native capture
  (`crash/native.rs`) allocates inside the unhandled-exception filter and records the
  handler's stack; Unix capture (`crash/unix.rs`) is signal-safe but has no stack or log.
  Three file-name schemes exist; reports carry no commit, session, thread or uptime; nothing
  reads them; there is no minidump or dialog.
- Release Windows builds use the GUI subsystem, so `main` failures such as missing carriers
  reach only `client.log`.
- `bedrock-core` is a separate process. Its stderr appends to `core.log`, bounded only at
  spawn. Rust discards its exit status (`core_process.rs:81`); `RestartBackoff` is unused.
  The auth and catalog helpers send stderr to null. Go never calls `slog.SetDefault`,
  `debug.SetCrashOutput` or sets a level.
- Redaction exists three times: Go `control.RedactError`, Go `secretSafeResourcePackHandler`,
  Rust `tools/devtool/src/diagnostic_bundle.rs`.
- `tools/bedrock-replay` replays `raw.bin` captures (`u32 id`, `u32 length`, body), but no
  tool in the repository produces them.
- Release profile has no `debug`, `split-debuginfo` or symbol artifact; `package.yml`
  publishes none.

## Performance budget

Default configuration must stay inside the frame, join and streaming budgets in
`docs/agents/live-testing.md`. The estimates below are unmeasured until the stated capture
runs; each phase PR attaches matched before/after captures for the paths it touches.

| Component | Default cost (estimate) | Guard |
|---|---|---|
| Early subscriber replacing `LogPlugin` | Same as today | Same layers; filtered callsites stay cached as never-enabled. |
| Reloadable filter | One uncontended read lock per *enabled* event | Disabled callsites are rebuilt through the interest cache, not checked per call. |
| Session spans and `ErrorLayer` | Per span creation, a few hundred ns | Spans only at session, join, pack, dimension and stream boundaries; hot-path spans stay behind `tracy` or at `TRACE`. |
| Lossless `WARN+` | Zero until the queue is full | A reserved queue slice for `WARN+`; only `ERROR` blocks, bounded at 50 ms. |
| Flight recorder | A memcpy per recorded event | Records events already enabled for the file; verbose categories are opt-in. |
| Crash handler | Zero while running | Exception filter and signal handlers only. |
| Crash monitor | Spawn off the critical path; an idle process of a few MB | Connection happens on a background thread; state messages only on state changes. |
| Hang watchdog | One relaxed atomic store per frame | One sleeping thread. |
| Packet header ring | Tens of ns per packet | Fixed-size records; no allocation or formatting. |
| Full packet capture (opt-in) | Reference-count clones; writes on a background thread | Bounded queue; a full queue records a gap, never stalls the network thread; size cap. |
| Debug categories enabled | Formatting each extra event, about 1-3 µs | Opt-in; rate-limit helpers on high-volume sites. |
| Typed secrets | Zero | Display prints a placeholder; no regex on the log path. |
| Symbols | No runtime cost | Split files are never loaded. Release link time grows; record CI timings. |

## Ownership

| Owner | Responsibility |
|---|---|
| `crates/diagnostics` | Subscriber construction, categories, file sinks, session rotation, flight recorder, rate-limit macros, `Secret<T>`, packet header ring types. Remains Bevy-free. |
| `crates/crash-report` (new) | Report schema, folder layout, write/read/prune, log-section extraction and share-time scrubbing rules. Used by the client, monitor and `devtool`. Bevy-free, no network dependency. |
| `crates/launcher-host` | Crash-handler installation, monitor spawn and IPC client, monitor mode, dialogs, core exit capture. Replaces `crash/native.rs` and `crash/unix.rs`. |
| `app` | Disables `LogPlugin`, adds Bevy-only layers (Tracy), publishes game state, frame heartbeat, settings and F3 rows. |
| `crates/client-session` | Packet header ring and opt-in capture at session ingress. |
| `core/` | slog configuration, crash output, version stamp. |
| `tools/devtool` | `diag` reading the new layout, `symbolicate`. |
| `tools/bedrock-replay` | Reads capture v1 and v2. |

New crates and edges are registered in `tools/architecture/policy.toml`; new dependencies
are recorded in `THIRD_PARTY_NOTICES.md`.

## Constraints

- One PR per phase into `dev`. Each bug fix carries a regression test that failed before.
- Tests never sleep; crash tests use subprocess fixtures following
  `crash/unix.rs::signal_fixture`, with `sadness-generator` triggering faults.
- Domain crates take `now` from callers; only the edge reads clocks.
- Every new environment variable is a constant in its owner and a `[[markers]]` entry;
  categories, debug flags and their documentation derive from one registry.
- Parsed acceptance markers keep their exact text and stream.
- Reports and logs never contain tokens, content keys, or full install paths.

## Phase 1: Logging foundation

**Files:** `crates/diagnostics/src/{logging.rs,logging/*}`, `crates/diagnostics/src/console.rs`,
`app/src/app/logging.rs`, `app/src/app.rs`, `app/src/lifecycle/{mod.rs,client_logs.rs}`,
`app/src/tracy.rs`, `crates/env-compat`, `crates/first-run/src/prepare.rs`,
`tools/architecture/policy.toml`, root `Cargo.toml`.

1. Build the subscriber in `diagnostics::logging::install` and call it first in
   `before_run`, and in every process mode that never builds a Bevy `App`
   (`--first-run-setup`, `--crash-monitor`). Layers: reloadable `EnvFilter`, file/console
   formatter, `ErrorLayer`, and an optional caller layer (Tracy). Install `LogTracer`.
   Reproduce the Tracy `frame_mark` filter and, on Android, the logcat layer. Disable
   `LogPlugin` in `app.rs`. The `diagnostics::console` writer thread stays the only writer;
   `tracing-appender` is not added.
2. Category registry: `net`, `world`, `render`, `ui`, `packs`, `audio`, `input`, `core`,
   `session`, each mapped to its targets in one table. `CINNABAR_LOG` accepts category names
   and raw `EnvFilter` directives; `RUST_LOG` keeps working. A test fails when a workspace
   crate with a `tracing` dependency belongs to no category.
3. File format: timestamp, level, thread name, target, span context, message, fields.
   Optional JSONL sink (`CINNABAR_LOG_JSON=1`) for tooling.
4. Session files: the current session stays `logs/client.log`; previous sessions move to
   `logs/client-<UTC timestamp>.log`, bounded by count (10) and total bytes (64 MiB). The
   `CLIENT_SESSION_START` header gains session ID and build profile.
5. Lossless policy: reserved queue capacity for `WARN+`; `ERROR` blocks up to 50 ms when
   even the reserve is full; dropped lower-level writes are counted and reported. Copy raw
   stderr into the file in terminal launches too.
6. Runtime control: `diagnostics::logging::set_filter` behind developer-control/MCP; a
   Settings → Advanced "Verbose logging" option applied at next launch; F3 shows the active
   filter.
7. `diagnostics::rate` macros (`log_every!`, `warn_once!`, per-site budget); migrate the
   hand-rolled limiters in `portal_diagnostics.rs`, `render/src/chunk/draw.rs`,
   `runtime_profile_slow.rs`, `telemetry/attribution.rs`, `environment/diagnostics.rs`,
   `block_overlay/textures/diagnostics.rs`, `blob_cache_telemetry.rs`.
8. Migrate production `eprintln!`/`println!` and non-marker `log_stderr!` calls to
   `tracing`; enable `clippy::print_stdout`/`print_stderr` in `[workspace.lints.clippy]` with
   reasoned `#[expect]` at true stdout contracts (`--query-build`, markers, tty prompts).
9. Bound `first-run.log` through the same rotating sink.
10. `Secret<T>` in `diagnostics`; wrap tokens, content keys and session secrets at their
    owners.

**Acceptance:** a `tracing::warn!` from `before_run` reaches `client.log`; a queue flood
loses no `WARN+` line; categories filter as documented; Tracy and Android builds keep
their layers; matched frame captures show no regression at default settings.

## Phase 2: Unified crash report

**Files:** `crates/crash-report/*` (new), `crates/launcher-host/src/lifecycle/crash.rs`,
`tools/devtool/src/diagnostic_bundle.rs`, `packaging/README.md`.

1. Folder per crash: `crashes/<UTC timestamp>-<id>/` with `report.json`, `client.log`
   (this session's section), `core.log` (this session's section), `flight.log`, and later
   `crash.dmp`. Bounded to 8 folders; pruned at startup and before each write.
2. `report.json` schema version 1: kind (`panic`, `native`, `abort`, `hang`, `core`),
   message, thread name and ID, backtrace, release, commit, build profile, OS, arch,
   GPU and driver, uptime, session ID, renderer attempt, game state, span trace, user
   comment.
3. Panic hook writes through `crash-report` with the client log section; one backtrace
   capture per panic. Regression test: the panic report contains the client log, not
   `core.log`.
4. `devtool diag` reads the new layout.

## Phase 3: Crash monitor and dialog

**Files:** `crates/launcher-host/src/lifecycle/crash/*`, `app/src/entry.rs`,
`app/src/lifecycle/mod.rs`, `crates/launcher-host/src/native_dialog.rs`,
`crates/launcher-host/src/renderer_startup/context.rs`.

1. `cinnabar --crash-monitor <ipc name> <parent pid>` runs a `minidumper` server, watches
   the parent handle and exits with it.
2. The client spawns the monitor first in `before_run` and connects on a background thread;
   until connected, Phase 2 in-process reports remain the fallback.
3. `crash-handler` replaces `crash/native.rs` and `crash/unix.rs`; the crash callback only
   requests a dump over IPC. The panic hook also requests a dump of the panicking thread.
4. Game state is pushed to the monitor when it changes: renderer attempt (replacing the
   leaked snapshot in `renderer_startup/context.rs`), session phase, server kind, pack count,
   dimension. Renderer crash exclusions keep working from the startup sentinel.
5. Exit without a dump (fail-fast abort, OOM kill, external kill) produces a `kind: abort`
   report from the exit status and logs.
6. Dialog: summary, expandable details, Open folder, Copy report (scrubbed Markdown for a
   GitHub issue), Restart, Close. Optional comment saved into `report.json`.
7. `main` failures in GUI-subsystem builds, including carrier errors with their path and
   rebuild command, show the same dialog.
8. Next launch shows an in-game notice for an unshown report (Android, or when the monitor
   itself died).

**Acceptance:** subprocess fixtures for panic, null dereference, stack overflow, abort and
external kill each produce exactly one report folder; native faults include a minidump
that `minidump-stackwalk` walks to the faulting frame.

## Phase 4: Hang watchdog

The main thread stores a frame counter each frame; the monitor reads it through shared state.
No progress for 10 s (loading screens excluded) writes a `kind: hang` report with a dump of
all threads and keeps the process running. One report per stall.

## Phase 5: Go core

**Files:** `core/cmd/bedrock-core/main.go`, `core/internal/lifeline`, `Makefile`,
`.github/workflows/*.yml`, `crates/launcher-host/src/{core_process.rs,launcher_core.rs,auth.rs,catalog.rs,lifecycle/core_health.rs}`.

1. `slog.SetDefault` with one handler, a `process` attribute and a level passed by Rust as
   `-log-level`, resolved from the `core` category. Replace raw `fmt.Fprintf` diagnostics.
2. `debug.SetCrashOutput` to a file path passed by Rust; `GOTRACEBACK=all` for crashes.
3. Stamp version and commit with `-ldflags -X`; report them at startup.
4. Rust pipes core and helper stderr through a reader thread into a rotating `core.log` with
   a per-process label, enforcing the size cap mid-session.
5. Rust records the exit status; a nonzero exit becomes a `kind: core` report with the Go
   crash output and both log sections. Wire or delete `RestartBackoff`.

## Phase 6: Redaction

1. `Secret<T>` in Rust and a `LogValuer` secret type in Go cover credentials at write time.
2. One share-time rule file in `crash-report` with test vectors; a byte-for-byte copy embedded
   in `core/internal/redact`, checked by a `crash-report` test. Go `redact.Error` (replacing
   `RedactError`), the Go resource-pack handler, `devtool diag` and Copy report use it.
   Placeholders unify to `[redacted]`.
3. Unredacted Go sites (`proxy.go:195-202`, `targets.go:69`, `proxy.go:251`, `main.go:159`,
   `friend_host.go:209`, raw upstream errors) move to typed or scrubbed fields.

## Phase 7: Packet diagnostics

**Files:** `crates/client-session/src/session/*`, `crates/diagnostics`,
`tools/bedrock-replay/src/capture.rs`, settings owners.

1. Header ring at session ingress and egress: last 4,096 records of direction, ID, length,
   tick and decode outcome. Dumped into each crash report.
2. Decode failures keep up to four raw samples per packet ID per session in the report folder.
3. Opt-in capture (`CINNABAR_PACKET_CAPTURE`, Settings → Advanced "Record network session")
   writes capture v2: a header with magic, version, protocol version and start time, then
   records with direction and timestamp. Size cap 512 MiB; a full queue writes a gap record.
   Confirm that session ingress sees no Login or pack-key packets; strip any that it does.
4. `bedrock-replay` reads v1 and v2; v2 timestamps drive an original-timing schedule.

## Phase 8: Symbols

1. `[profile.release]` gains `debug = "line-tables-only"`, `split-debuginfo = "packed"` and
   `strip = "debuginfo"`; record release build time before and after.
2. `package.yml` uploads per-platform symbol archives (PDB, dSYM, `.dwp`) as release assets
   keyed by version and commit.
3. `devtool symbolicate <crash folder>` downloads or reads the matching archive and walks
   the minidump with `minidump-processor`.

## Documentation

- `docs/architecture/diagnostics.md`: categories, files, report layout, packet capture,
  sharing.
- `docs/agents/live-testing.md`: `CINNABAR_LOG`, capture and full-dump flags.
- `packaging/README.md` crash section; `docs/architecture/workspace.md` crate table;
  `plan.md` ledger entries per phase.

## Rollout

| Wave | Pull requests | Depends on |
|---|---|---|
| 1 | Phase 1; Phase 2; Phase 5 Go side (slog default, `-log-level`, `-crash-output`, version stamp); Phase 7 recorder and capture; Phase 8 | `dev` |
| 2 | Phase 3; Phase 5 Rust side (flags, stderr pipe, exit reports); Phase 6; packet ring in crash reports | Wave 1 merges |
| 3 | Phase 4; documentation and `plan.md` ledger | Phase 3 |
