# Live testing, capture, and performance evidence

Read this before capturing rendered frames, profiling the client, or closing a
visual or performance acceptance gate.

## Scripted client sessions

Use the [client MCP](client-mcp.md) with `headless: true` and the
`developer-control` feature for automated input, screenshots, and recordings.
Use `cargo run -p ui-snapshot -- --list`, then `--state NAME --out /tmp/ui.png` for isolated UI states.
Fixtures live in `tools/ui-snapshot/src/fixtures.rs`; PNGs have viewport/backend JSON sidecars and use `.local/assets/compiled` (or `--carriers DIR`).
Use `--time SECONDS` to compare fixed animation phases, including cosmetic gallery tiles.
Missing carriers report a named skip; no game or window runs. Do not inject OS input
or open a visible window for automated captures. Use a local server by default;
remote joins require the owner's authorization because they use the configured
account.

Keep account fixtures, generated carriers, screenshots, and profiler outputs in
isolated local or temporary directories. Record the build, game/server version,
scenario, platform, resolution, and scale alongside each acceptance result. Inspect
fresh output from that run. A missing capture is incomplete validation.

Headless sessions render through a hidden window. They can verify pixels and
rendering work, but displayed-frame pacing, OS focus, and occlusion gates require
appropriate presentation evidence and remain incomplete without it.

A signed-in capture account still receives Xbox join requests and other toasts, which `F1` does
not hide. `--hide-toasts` (`developer-control` builds only) stops drawing toasts for the session
so they cannot land in a comparison capture; they still queue and expire as usual.

To compare frames against another renderer at a known time of day, launch a `developer-control`
build with `--fixed-time <TICKS>` (0 = sunrise, 6000 = noon, 12000 = sunset, 18000 = midnight).
The sky and lighting then draw at that time for the whole session, ahead of any mod's time
changer; the server clock, and so gameplay, keep running. Release builds reject the flag.

Server compatibility tests can establish connection, transfer, or server-authority
behavior. They do not by themselves establish version-matched Bedrock parity.
Choose a reproducible scenario and distinguish server behavior from client output.

## Visual acceptance

A UI, HUD, text, graphics, shader, or rendering change is not ready to push or
describe as done without a real rendered-frame pass on the target platform,
resolution, and DPI/scale. Unit tests, snapshots, draw-list checks, GPU adapter
tests, lint, and code review are necessary but never substitutes for seeing the
output. The pass must explicitly check legibility, geometry, clipping,
depth/layering, scaling, colors, and the relevant live input/focus behavior, and
must record the tested platform and visible result. If the target-platform pass
cannot be performed, keep the change local and say it is not cleared to push.

## Frame budgets

Release gates are 120 Hz on the reference M3 Pro (1920×1080, 12-chunk radius) and 60 Hz on a
low-end laptop (i5-8250U, UHD 620, 8 GB dual-channel, SSD; 1280×720, 8-chunk radius), with a
separate 240 Hz tier. Medians and p99s in milliseconds:

| Tier | Main CPU | Render CPU | GPU | Hard hitch |
|---|---:|---:|---:|---:|
| M3 Pro, 120 Hz | 2 / 4 | 2 / 4 | 4 / 6 | ≥ 16.67 |
| Low-end, 60 Hz | 4 / 8 | 4 / 8 | 8 / 12 | ≥ 33.33 |
| M3 Pro, 240 Hz | 1 / 2 | 1 / 2 | 2 / 3 | ≥ 8.33 |

Other refresh rates scale the 120 Hz row by 120/H. The stages overlap in the pipeline: each must
fit its own budget, and they are never summed. CPU time excludes intentional pacing waits. Lower
hardware gets lower resolution or view distance, never permission to stutter.

- **Hitches.** With T = 1000/H, a displayed interval over 1.5T is a hitch and one of 2T or more is
  a hard hitch. Qualification captures allow zero hitches caused by our code; p99 grants no 1%
  allowance. First-use shader or pipeline compilation during play is a defect: compile while
  loading or before the object becomes visible. OS preemption is excluded only with scheduler
  evidence. Every unexplained hitch gets a trace.
- **Streaming.** Sustain 128 blocks/s flight (a chunk boundary every 125 ms) with 32 blocks of
  ready terrain beyond the visible edge. Chunk request to render-ready, including GPU upload,
  must stay at or under 125 ms p99, and a complete payload to render-ready at or under 50 ms p99,
  with zero missed visibility deadlines and processing capacity of twice the arrival rate. Record
  server starvation separately.
- **Joins.** With authentication and packs cached, on a LAN server, Join click to the first
  controllable frame with complete visible terrain takes 500 ms median and 1 s p99 on the M3 Pro,
  and 1 s and 2 s on the low-end tier. With an empty shader cache: 4 s and 8 s p99.
- **Captures.** Use a pinned replay of a 30-player lobby with signs, heads and banners, plus a
  terrain-streaming route, plugged in after thermal stabilisation. Take three 120 s runs per
  build, with cold-cache runs kept separate, capturing every frame, GPU timestamps, presentation
  intervals and thread waits. Report p50, p99, maximum, deadline misses and hitch counts.
- **CI.** Assert work, never milliseconds. A warm, unchanged input does zero attributable heap
  allocations, geometry or text rebuilds, pipeline creations and redundant upload bytes. A change
  rebuilds only its dependents, once per object per publication. Streaming queues stay bounded,
  obsolete work is cancelled, and no chunk version is processed twice.
- **Slow-frame trigger.** `CINNABAR_SLOW_FRAME` fires on an interval over 1.1T (9.17 ms at
  120 Hz, 18.33 ms at 60 Hz) or a frame over its main CPU, render CPU or GPU budget above. T comes
  from the window's current monitor, or a slower frame cap. Text is rate-limited; every event is
  counted, marked in the frame trace and totalled as `slow_frames` in `CINNABAR_STAGE_PROFILE`.

## Tracy frame attribution

Tracy is the standard interactive frame/stall trace. `make play TRACY=1` enables
it for manual sessions. For scripted sessions, build with
`--features developer-control,tracy` using the [build workflow](multi-agent-workflow.md)
and launch through the MCP with `headless: true`, connecting with `local_server`.
Keep INFO spans enabled. The feature is off by default; its subscriber, zones and
GPU plots are absent from ordinary builds. Domain crates remain Bevy-free.
Zone names stay fixed; changing counters and job IDs appear as zone text to avoid
exhausting the collector’s source-location table.

Install the capture tools from the Tracy release whose protocol matches
`tracy-client-sys` in `Cargo.lock`: `brew install tracy` on macOS when Homebrew has
that release, or the release's `windows-*.zip` on Windows. The
[rust_tracy_client table](https://github.com/nagisa/rust_tracy_client#readme) maps each
`tracy-client-sys` version to its Tracy release; mismatched tools refuse the connection.
With the hidden local scene settled:

```sh
TRACE_DIR="$(mktemp -d)"
tracy-capture -a 127.0.0.1 -o "$TRACE_DIR/cinnabar-frames.tracy" -s 120
tracy-csvexport -u "$TRACE_DIR/cinnabar-frames.tracy" > "$TRACE_DIR/cinnabar-zones.csv"
```

Keep captures and exports outside git. The client accepts only loopback capture
connections and records on demand; source transfer, broadcast, sampling and
system tracing are disabled.
Never collect or export process environments. Export zones/plots, not metadata.

The frame bar marks post-present intervals; completion submissions and cleanup may
follow each marker. Select a long interval, then expand the main/render threads and
nested schedules, systems and graph nodes (their identities are in zone text).
Inspect `stream.*`, `light.*`, `mesh.*`,
`*.upload*`, `*.device_poll`, readback and completion zones, plus Bevy's
`prepare_windows`, `submit_graph_commands` and `present_frames`. Worker-wait and
queue-lock zones identify explicit waits; a long upload/poll is elapsed API time,
not proof of active CPU work. macOS Tracy has no scheduler trace; use separately
correlated native scheduler evidence before assigning preemption or lock owners.

The pinned Bevy `trace_tracy` bundles a GPU recorder whose calibration uses encoder
timestamps and waits for completion. Metal does not support that path. Our feature
uses Bevy `trace`/`debug` and the same Tracy layer without that recorder. Existing
Metal pass queries appear as delayed `elapsed ms (readback)` plots, not GPU timeline
zones; they overlap and cannot be added or matched to the receipt frame. Clock
anchor zones bound the sampled wall/game-clock relationship for trace comparison.

## Client logs

Each launch writes `logs/client.log` (`.local/logs/` in a checkout), starting with a
`CLIENT_SESSION_START` header that names the version, commit, build profile and session
ID. Raw stderr and panics reach it in terminal launches too. The previous session moves to
`client-<UTC yyyymmdd-hhmmss>.log`; the newest 10 archives, up to 64 MiB, are kept, and a
session past 8 MiB continues in a fresh file under the same header. Warnings and errors
keep reserved console queue space, so a flood of lower-level lines cannot drop them; dropped
lines are counted in the log.

`CINNABAR_LOG` layers comma-separated directives over the defaults and `RUST_LOG`: a
category with a level (`net=debug`), a bare category for every level, or any `RUST_LOG`
directive (`wgpu_core=warn`). Invalid directives are skipped with a warning in the log. The
client MCP `log_filter_set` and `log_filter_get` tools change and report the same spec at
runtime. `CINNABAR_LOG_JSON=1` also writes each event as one JSON object per line to
`logs/client.jsonl`. Settings → General → Advanced → Verbose logging applies
`net=debug,session=debug,core=debug,packs=debug` at launch and when toggled, unless
`CINNABAR_LOG` or `RUST_LOG` is set. F3 shows the active filter.

Every client also starts a `--crash-monitor` process that exits with it and logs to
`logs/crash-monitor.log`. A crash, abort or kill opens the crash window once the client is gone;
a panic the client survives only writes its folder. A main loop that finishes no frame for 10 s
outside a world join writes a `kind: hang` folder while the client keeps running; a client paused
in a debugger is exempt. Set `CINNABAR_CRASH_UI=0` for scripted and
headless sessions so a crash only writes its `crashes/` folder, and `CINNABAR_FULL_DUMP=1` for a
full-memory `crash.dmp` (Windows).

| Category | Covers |
| --- | --- |
| `net` | Protocol, web transport and client network runtime |
| `world` | World data, chunk streaming, meshing, simulation and local player |
| `render` | Renderer, wgpu, presentation, camera, particles and environment |
| `ui` | JSON-UI, OreUI, HUD and screens |
| `packs` | Assets, resource packs, pack runtime and extensions |
| `audio` | Audio playback and decoding |
| `input` | Input mapping, gamepads and windowing input |
| `core` | Go core bridge, launcher, lifecycle, crash monitor and diagnostics |
| `session` | Client session, join, rich presence and telemetry |

## Native and performance evidence

Use native Bedrock/BDS comparison when it decides a contract or closes an explicit
acceptance gate, preferring version-matched, reproducible, fixed-state galleries and
exact protocol fixtures over visual guesswork. Test the integrated build and record
any platform or capture limitations. Batch equivalent captures and reuse an
authoritative existing witness when it covers
the same version, state product, camera, geometry, material, and behavior question.
Performance claims require measured release evidence against the budgets above; a debug screenshot, small test scene, or green unit suite is not
performance acceptance.

Diagnostic variables use the `CINNABAR_` prefix; the client still reads the old `RUST_MCBE_`
spelling for one release and logs a deprecation warning.

For main-thread attribution, set `CINNABAR_STAGE_PROFILE=1` only on an
instrumented release acceptance run. The client emits one
`CINNABAR_STAGE_PROFILE` record per second with count, cumulative milliseconds,
and maximum milliseconds for each runtime stage; `main_frame` is the main world's
frame-start-to-`Last` wall time, including stream reclaim. The chunk, actor, UI,
particle, audio and block-entity stages attribute that time. Compare runs with the same
scene, BDS state, duration, release profile, and present mode. Treat overlapping
worker and main-thread stages as attribution rather than additive wall time, and
run the final performance gate again without the variable because profiling
changes the measured workload.

Normal play also emits `CINNABAR_SLOW_FRAME` without any environment setting,
on the trigger in Frame budgets, at most once per second with the number of
suppressed slow frames and the violated budgets. `main_ms` covers frame-start timing to
`Last`; `between_updates_ms` covers the rest of the preceding start-to-start
interval. `main_stages` lists spans completed during that update, and
`window_stages` includes the intervening render work, both in descending
milliseconds. `main_pre_update`, `main_fixed_update`, `main_update` and
`main_post_update` split `main_frame` by schedule phase, so work no named stage
covers still shows which phase spent it. These are overlapping wall-time spans,
not additive CPU or GPU time. A worker span can begin in an earlier frame; its full duration appears in
the window where it completes. `surface_preparation` includes drawable
acquisition and schedule overhead; `render_submission` includes CPU render
graph execution, queue submission and presentation. A large interval with
small main work warrants checking the render stages and OS scheduling before
changing gameplay. `render_frame` is render-world time excluding drawable acquisition.
`input_age` runs from the update's start, where input is sampled, to the end of its render
extraction. `input_pacing_wait` is the deliberate main-thread delay before the next input sample, until the
later of its frame-rate cadence slot and the predicted render-thread completion;
`frame_pacing_lateness` is how far past that deadline the wait woke. `CINNABAR_INPUT_PACING=0`
stops only the render-completion delay; a frame-rate cap still applies.
`world_service` is the world-stream service thread's polling time between frames, recorded
at reclaim; it overlaps extraction and pacing and is not main-thread time. Trace exports
show it as a background-work counter at receipt, not a continuous execution span.
`world_service_reclaim` is the main-thread wait to take the stream back before `First`.
Both `main_frame` and input pacing's main-thread estimate include that wait.
`CINNABAR_WORLD_SERVICE=0` keeps every world-stream poll on the main thread, a
same-binary control for chunk-streaming comparisons.

GPU timing uses timestamp queries when the adapter supports them, read back
asynchronously, so `gpu_*` stages describe a frame a few frames older than the
window they appear in. `gpu_frame` spans the first to last timestamp only when the sampled graph covers the frame.
Without the stage profiler, frames record only `gpu_frame`, from the first timed node to the end of
the frame's graph, where the frame's own submission resolves its spans; a shown F3 overlay times every
pass in one frame per refresh.
On Metal, queries attach to existing owned render passes and the stock opaque pass:
other stock Bevy passes and shared draw categories remain unmeasured. Empty
compute marker passes do not produce usable timestamps on Apple GPUs, so `gpu_frame` instead
spans the GPU host times of empty marker command buffers that open and close the graph's
submission: from the later of submission and the end of earlier queued work to the end of the
frame's work. Pass categories
sum elapsed latencies, including gaps and overlapping GPU work; they are not GPU
active time and must not be added. Invalid timestamp pairs are skipped, so a
category may cover only some of its passes. Check coverage against native traces;
`CINNABAR_GPU_QUERY_HEALTH=1` logs per-stage invalid-pair and readback counters once a second.
Node stages
are `gpu_shadows`, `gpu_opaque`, `gpu_transparent`, `gpu_ui`, `gpu_hand`,
`gpu_post`, `gpu_tonemapping`, `gpu_fxaa` and `gpu_blit`. During
per-pass timing outside Metal, the GPU-cull terrain path also records its existing
compute passes as `gpu_cull_early`, `gpu_cull_hiz` and `gpu_cull_late`, and its
late terrain render pass as `gpu_terrain_late`. These stages use one query pair per
pass without extra passes or submissions; they appear in F3's sampled frames and
`CINNABAR_STAGE_PROFILE` aggregates. Metal keeps its existing owned-pass timing. With
`CINNABAR_STAGE_PROFILE=1` on adapters with in-pass timestamps (not Apple GPUs),
draws add `gpu_terrain_solid`, `gpu_terrain_cutout`, `gpu_terrain_model`,
`gpu_terrain_depth_liquid` (direct and CPU-planned indirect draws; GPU-cull terrain
commands expose no in-pass spans), `gpu_terrain_transparent`, `gpu_actors`,
`gpu_particles`, `gpu_sky`, `gpu_panorama` and `gpu_mod_primitives`. Personal-mod post passes
add `gpu_mod_pass_0`–`gpu_mod_pass_7` by execution slot. F3 shows the latest GPU frame.
`gpu_idle` is the GPU gap from the previous timed frame's last timestamp to this frame's
first, reported only when both consecutive frames were timed; untimed work between them,
such as uploads and presentation, counts as idle.

`CINNABAR_GPU_CATEGORIES=1` splits the main opaque phase into category passes
and disables in-pass spans to avoid double counting; separate late GPU-cull draws
retain their whole-pass timing outside Metal. `CINNABAR_OPAQUE_LAYERS=1` measures submitted alpha-surviving
terrain coverage in a separate raster target. Both are diagnostic workloads;
keep them out of ordinary performance captures. See the
[opaque foliage fixture](../../tools/localserver/opaque-overdraw.md). Fast frames use fixed-size counters without formatting or
file I/O; aggregate snapshots and full traces remain opt-in.

With the `tracy` feature, `CINNABAR_GPU_NODES=1` times every render-graph node with
encoder timestamps and plots each as `gpu node <graph>/<label> ms`, plus named sections
inside the late GPU-cull node as `gpu section <name> ms`; it adds a timestamp pair per node, so
take frame-time numbers from a capture without it. Tracy builds also enable wgpu's own
zones (`CommandEncoder::finish`, `Queue::submit`, `RenderPassInfo::start`, the DX12 present
and barrier calls), which is where the render thread's encoding time shows: wgpu records
passes and encodes them at `finish`, and the opaque, late-cull and transparent passes
finish on worker tasks.

`CINNABAR_GPU_UI=1` replaces the aggregate UI timestamp category with
`gpu_ui_raster`, `gpu_ui_model`, `gpu_ui_composite`, `gpu_ui_invert` and
`gpu_ui_backdrop` (the shared scene blur for mod-surface glass) on owned passes. It also reports submitted pass, draw and index counts, layer reuse,
target extent/format/sample count, and geometry, texture and viewport upload bytes
once per second. Metal timings retain the latency and invalid-pair limitations
above; a missing category is not a zero-cost pass. Pair this diagnostic with query
health and repeat the ordinary capture without it before making a performance claim.
With these diagnostics enabled, `CINNABAR_GPU_UI_BASELINE=1` selects the previous
replay policy: retain identical revisions and fully redraw newer publications.
Both modes retain unchanged viewport uniforms. Use this same-binary control
for matched comparisons, with equal warm-up and alternating run order. The switch
is sampled at startup and has no effect without UI diagnostics.

`CINNABAR_GPU_MEMORY_REPORT=<seconds>` logs the device allocator's allocated and reserved
totals at that interval (target `cinnabar::gpu_memory`), its largest allocations summed by
label, and the memory blocks with the most reserved-but-unused room with what holds each.
Backends without an allocator report (Metal, GL) log nothing.

After the startup visibility probe stops, ordinary world-publication logs keep
current stream and upload counters but mark the inactive visibility witness as
`visibility_snapshot_valid=false`, with null frame, pose, view and draw-mode
identities. Frozen startup visibility markers are no longer printed as current
gameplay evidence. Explicit acceptance and metrics probes retain their existing
snapshot schema.

For frame-level attribution, also set `CINNABAR_STAGE_PROFILE_FRAMES` to a
scratch JSON path. The profiler keeps a bounded, preallocated trace in memory
and writes Chrome trace events on the exit frame. It includes window focus and
OS occlusion at each main-frame boundary, executing threads, session geometry,
artwork and equipment setup, pack reload, world polling, and render surface
preparation. No trace file is written during normal updates. A full recording
drops subsequent events and sets `truncated`; inspect this flag before choosing
a measurement window. Surface preparation includes schedule overhead and is an
upper bound on drawable acquisition, not proof that macOS caused a stall.

Build with the `cinnabar/frame-trace` feature for named Bevy schedules,
systems, graph nodes, command generation, queue submission and presentation spans.
The client's subscriber writes the Chrome trace to `TRACE_CHROME`, or a timestamped
file in the working directory, and changes the workload; use a short diagnostic run,
separate from the matched long captures.

`CINNABAR_STAGE_PROFILE_EVENTS` sets the bounded event budget (default 131,072;
maximum 2,097,152). Invalid values retain the default. Exported traces include the
capacity, exact dropped-event count, a wall-clock anchor and game-clock frame
anchors. GPU counter events carry a readback sequence and duration in nanoseconds;
their timestamp is receipt time, not GPU execution time or a CPU-frame identity.

Keep native profiler bundles outside git. Export only explicitly selected timing
and stack tables; never export the trace table of contents or process metadata,
which can include environment variables. Use scheduler states and sampled stacks
to separate blocked time from runnable delay and active work. Profiling overhead
and unavailable GPU categories must be reported separately.

Developer client builds include the default `acceptance` feature. Packaging opts out explicitly;
use a developer build for acceptance evidence, even when testing with the release profile.

## Packet capture

The client keeps the last 4,096 packet headers of its sessions in both directions (ID, length,
the time the client read it and whether ingress accepted, skipped or failed the packet) and up
to four raw bodies of each rejected packet ID, 1 MiB in total, from the latest session that
rejected one. Beside them a log flight recorder keeps the last 2,000 log lines its own filter
passes: `debug` for the `net`, `session`, `core` and `packs` categories and the default level
elsewhere, whatever `CINNABAR_LOG` sets for the session file. All three live in the session's
crash staging directory, `crashes/.live-<session id>/` (the header and log rings as mapped
files, the samples as written), so every crash folder gets `packets.txt` (the header table with
packet names), `decode-samples/` and `flight.log`, even after a native fault or an external
kill. Staging directories of clients that are gone are deleted at the next startup.

Set `CINNABAR_PACKET_CAPTURE=1` (or turn on Settings → General → Advanced → Record network
session) to record each session in full to `logs/packets-<session start unix ms>.cnbrcap`, or
set the variable to a file path. A set variable, including `0`, overrides the setting. An existing file is
never replaced; the start time is added to the name instead. Before each capture into `logs/`, the
oldest `packets-*.cnbrcap` there are deleted so the folder keeps at most 10 captures and 2 GiB,
counting the new one at its 512 MiB cap; a capture at a path you named is never deleted. The capture uses the v2 format that
`tools/bedrock-replay` reads, with `-original-timing` replaying the recorded times; those are
when the client read each batch, which trails socket arrival while ingress is backlogged. It
stops at 512 MiB; a writer that falls behind records how many packets it dropped rather than
slowing the network thread. Session teardown finishes the file, waiting at most 2 s; a crash
loses at most the queued packets and 64 KiB of written ones and leaves a capture that
`bedrock-replay` reports as truncated. Login, handshake and resource-pack offer packets are left
out of captures and rejected-body samples, keeping only their ring header, so neither holds
credentials or content keys; captures still hold chat and world data, so keep them outside git
and delete them after use.

## Packet latency trace

Set `CINNABAR_LATENCY_TRACE` to an existing directory before launching the client; it and its core
each write `<role>-<pid>.trace` there. Both stamp events with Unix nanoseconds read once at
startup plus monotonic time since, so the files join on one clock. Inbound, they follow each
packet from the core's decoded relay batch through the bridge frame, world ingress sequencing, the
app's drain and commit, to the frame handoff and the presented frame; outbound, from the frame's
time sample through the PlayerAuthInput send, frame flush and bridge write to the core's RakNet
hand-off. The connection ID the core sends in its handoff, with bridge frame numbers and slots,
links the two files. Event kinds and fields are listed in `crates/bridge/src/latency_trace.rs`
and `core/internal/latencytrace`. Traces hold packet IDs, these identifiers and timings only, no
packet bodies.
