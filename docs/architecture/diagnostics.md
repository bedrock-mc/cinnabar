# Logging, crash reports and diagnostics

Every Cinnabar process writes bounded, local diagnostics. Nothing is uploaded; a player
shares a report only by copying it or attaching its folder. Operational details live in
[live testing](../agents/live-testing.md) (variables, categories, packet capture) and the
[packaging guide](../../packaging/README.md#crash-reports) (report contents and symbols).

## Owners

| Crate | Owns |
| --- | --- |
| `diagnostics` | The process subscriber (`logging::install`), log categories, the console writer thread, session log files, the flight recorder, rate-limit macros, `Secret`, and `session` identity. Bevy-free. |
| `crash-report` | The report schema, `crashes/<UTC time>-<id>/` folders, log sections, the packet header ring layout, and the shared redaction rules (`data/redaction-rules.json`). |
| `launcher-host` | Crash capture in the client (`crash-handler`, the monitor connection, game-state messages, in-process fallback writers), native dialogs, and the Go core's process wiring and `core` reports. |
| `crash-monitor` | The `--crash-monitor` process: minidump server, client watch, hang detection, report writing and the crash window. |
| `bootstrap-ui` | The pre-game window stack shared by first-run setup and the crash window. |
| `client-session`, `protocol` | Packet tap, decode-failure samples and opt-in capture. |
| `core/internal/redact`, `core/internal/corelog` | Go redaction (an embedded copy of the shared rules, checked for drift) and the core's root logger and crash output. |

## Logging

`diagnostics::logging::install` runs first in `before_run` and in every process mode that
never builds a Bevy `App`; Bevy's `LogPlugin` is disabled. It installs `LogTracer`, a
reloadable filter, the file and console formatter, the JSON sink, the flight recorder,
Android logcat and any caller layer such as Tracy. Categories (`net`, `world`, `render`,
`ui`, `packs`, `audio`, `input`, `core`, `session`) map to crate targets in one table; a test
fails when a crate that logs belongs to none. The console writer thread is the only writer:
warnings and errors use reserved record and byte capacity. Overflow is counted, and an error
waits only a bounded time for room. The Go core receives
its level from the `core` category and labels every line with its process role.

## Crash flow

The client starts the monitor first and connects over a private socket authenticated by a
token passed on the monitor's stdin. A fault or panic only requests a dump; the monitor
writes the folder after the client is gone so the logs include the final lines, then opens
the crash window. Exits without a dump (fail-fast aborts, kills) are reported from the exit
status. A panic the client survives is saved without a window. The monitor bounds pending
reports and, after the client ends, selects the first report still retained from its final
panic cascade. It reads that report's saved logs from the same folder. A main-thread stall outside
loading phases produces a `hang` report while the game keeps running. Before the monitor
connects, and on Android, in-process writers produce the same folder layout. iOS allows no
monitor process and `crash-handler` has no iOS fault handler, so there panics are written
in-process and native faults go unreported.

Every folder holds `report.json`, the session's `client.log` and `core.log` sections,
`crash.dmp` when a dump exists, `packets.txt`, `decode-samples/` and `flight.log`. The
packet and log rings are file-backed mappings in `crashes/.live-<session id>/`, so they
survive native crashes. Copy report and `devtool diag` scrub text with the shared rules.

## Cost

Logging at default levels costs the same as before. Disabled events cost about 2 ns; an
event kept only by the flight recorder costs about 1.7 µs (criterion, Windows, shared host).
Packet header recording is lock-free and allocation-free. The crash handler and monitor
cost nothing while the game runs apart from an idle process; the hang heartbeat is one
store per frame. Release builds keep line tables in split debug files, so binaries are
unchanged at runtime.
