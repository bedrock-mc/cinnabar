# Offline join replay

`bedrock-replay` sends saved server packets through the same private session endpoint used
by the Go core. Run the native client with `--socket-dir` and no `--address`.
The helper has no upstream server option. It rejects captured Transfer packets
before opening the listener. Captures, downloaded packs, traces and reports belong
outside git, preferably in a persistent evidence directory rather than `/tmp`.

## Build and run

Build the Rust workspace binary with the configured build-slot wrapper:

```sh
"$BUILD_SLOT" cargo build -p bedrock-replay
cp target/debug/bedrock-replay "$EVIDENCE/bedrock-replay"
"$EVIDENCE/bedrock-replay" \
  -capture "$FIXTURES/raw.bin" \
  -resource-pack "$FIXTURES/server.mcpack" \
  -socket-dir "$EVIDENCE/bridge" \
  -burst-packets 32 -burst-interval 50ms \
  -timeout 2m -report "$EVIDENCE/replay.json"
```

Wait for `BEDROCK_REPLAY_READY`, then start the native client in another terminal.
On macOS, use an isolated install layout so pack downloads, settings and logs do
not change the owner's install. The executable's location determines that layout, not the
working directory. A copied development binary under
`<isolated-root>/target/debug/bedrock-client` uses `<isolated-root>/.local`.
Prepare compatible carrier copies there and record their hashes.
On Windows, retain the repository's firewall-approved executable path and follow
`docs/agents/live-testing.md` before running the client.

```sh
RUST_MCBE_STAGE_PROFILE=1 \
RUST_MCBE_STAGE_PROFILE_FRAMES="$EVIDENCE/frames.json" \
"$ISOLATED_CLIENT" --socket-dir "$EVIDENCE/bridge" \
  --assets "$WORLD_CARRIER" --acceptance-seconds 15 \
  --metrics-out "$EVIDENCE/metrics.json"
```

Timed acceptance starts after world readiness. If a diagnostic fixture never
reaches that state, add `-hold 15s` to the helper: it closes the bridge 15 seconds
after the last burst. The native client treats that EOF as a session failure and
exits through AppExit, allowing the frame trace to flush. Record that native
nonzero status as an **expected fixture EOF**, exclude teardown frames, and do not
call it a successful gameplay acceptance run. A timeout or incomplete replay is
an error. A force kill or macOS termination watchdog may lose the native trace.

The client sends Connect and receives a Handoff containing the recorded startup
through StartGame. Pack archives follow as PackData; the remaining recorded
packets arrive as session batches. The first report burst covers startup as one
handoff, independent of the play burst limits. Later bursts use the requested
packet and byte limits. No second Minecraft login runs on the local connection.

## Fidelity and report interpretation

The input format repeats little-endian `u32 packet_id`, `u32 body_length`, then
the body. It has no timestamps or subclient fields. The helper reconstructs a
header with zero subclient IDs and preserves all selected bodies and their order.
Use a capture from the pinned protocol version; the report identifies the
protocol supported by the binary, not a version inferred from the capture.

Before the first StartGame, network settings, encryption, login status, pack info
and pack stack are replaced by the session handoff. Supplied local archives
are handed off in command-line order. Their hashes and identities are recorded;
the original offer, experiments, encryption and download timing are not reproduced.
Other pre-StartGame packets and all packets from StartGame onward are retained.
Exactly one StartGame is required. This exercises archive handoff and session
setup, but does not prove fidelity of the original server's login handshake.

Bursts use a fixed schedule starting after Connect. Each play burst is bounded by
both packet count and serialized bytes; packets are never split. When the socket
blocks, later bursts catch up to their original deadlines. The report records
each burst's source record range, scheduled time and actual flush time. Repeat
the same capture, packs, burst settings, client build, carrier hashes, cache state,
presentation settings and focus/occlusion conditions for before/after work.
Original network arrival timing cannot be recovered from this capture format.

`complete` means every selected packet was flushed and the ordered byte witness
matched. It does not mean the client consumed every packet or presented the world.
The witness hashes each full packet preceded by its little-endian `u32` length.
The raw capture has its own SHA-256. Reports are created exclusively: an existing
path is an error. `end_reason` distinguishes client exit from planned fixture EOF;
`error` records interrupted runs. Inspect native logs and trace `truncated` too.

The fixture ignores client gameplay requests and corrections. Captures that rely
on missing client-cache blobs or interactive server responses may not produce a
complete world. Synthetic fixtures, instrumented development builds and runs
under build load validate the harness and help attribution; they do not close a
native performance or vanilla parity gate.

## Frame-rate measurements

The client's metrics report includes `frame_sample_seconds`, the sum of recorded
frame durations, and `average_fps`, which divides `frame_count` by that sum.
These use the selected warmup/sample window; dividing by the entire process or
session duration would also include time outside that window.

`one_percent_low_fps` describes the reciprocal mean duration of the slowest one
percent of recorded frames, rounding the number of selected frames up. This is
different from the reciprocal of p99 frame time. Its `lower` and `upper` values
bound the result using the existing histogram; `sample_count` is the number of
slow frames selected. `frame_histogram_resolution_ms` reports the precision.
The exact maximum bounds overflow samples, so long hitches are not silently
clipped at the histogram ceiling. The value is null when no positive duration
has been recorded.

These are client update timings, not a count of frames displayed by the OS.
Retain the present mode, focus/occlusion, world-ready cohort, build profile and
host-load evidence alongside them. Acceptance mode also performs full-world
cohort checks that normal play stops after startup; account for that measurement
cost when attributing a steady-frame bottleneck.

## Tests and a minimal diagnostic fixture

```sh
cargo test -p bedrock-replay
CINNABAR_REPLAY_FIXTURE_OUT="$FIXTURES/repository-startup.bin" \
  cargo test -p bedrock-replay export_replay_fixture -- --nocapture
```

The exported stream uses the committed protocol StartGame, AddActor and Text
fixtures, checked against their manifest hashes. Their bodies are unchanged;
their batch wrappers and test subclient headers are removed. Generated empty
ItemRegistry, chunk radius and spawn-status packets complete the local login.
This is a synthetic sequence with one actor and no chunks, not a captured join.
The committed protocol LevelChunk deliberately has a dummy payload and is omitted.
Use `-hold` for a bounded native probe.
The tests compare packet bytes across the session transport, hand off an
original minimal pack and compare its hash, exercise both shutdown paths, and
reject truncated input, transfers, invalid bounds and reused report paths.
