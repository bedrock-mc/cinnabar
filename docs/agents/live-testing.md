# Live testing, capture, and native evidence

Load this before running the Bevy client or BDS on Windows, capturing frames, or
closing a native/visual/performance acceptance gate.

## Windows capture

Use native Computer Use/WGC as the primary path for Cinnabar window inspection and
input testing. Do not assume the Bevy window is inaccessible because an earlier run
failed: refresh app/window discovery for each live run and diagnose a missing
target as a current integration bug. If native capture genuinely fails after fresh
discovery and recovery, use Windows GDI `CopyFromScreen` only as an explicit
fallback, write PNGs beneath `%TEMP%`, and inspect those fresh files with the
image-viewing tool. Never claim visual verification from a stale or occluded
capture.

## Stable executable paths

Windows Firewall consent is path-specific, so reuse the paths the user already
approved: `.local/bds-runtime/bedrock-server-1.26.32.2/bedrock_server.exe` for BDS,
and `target/debug/bedrock-client.exe` for the Rust client (rebuild in place to keep
it stable). Do not copy either executable to a new worktree or temporary path for a
live run, do not change firewall policy, and do not automate UAC or security-consent
dialogs. If a genuinely new listening executable is required, explain why and wait
until the user is at the PC.

## Remote movement test targets

Use these user-designated Bedrock endpoints for Phase 3 movement and session
acceptance:

- Zeqa: `zeqa.net:19132`.
- Lifeboat: `play.lbsg.net:19132`. After joining, `/transfer sm3` exercises a
  deeper transfer/session path.
- Zeno external BDS: `zenomc.org:19197`. This is the low-population
  server-authority target for observing official-BDS movement rejection and
  correction behavior without depending on other players.

Treat these as compatibility and server-authority targets, not as substitutes
for a version-matched native Bedrock parity comparison. Record the resolved
endpoint, server-reported version, scenario, duration, and exact client build in
each acceptance artifact.

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
- **Slow-frame trigger.** `RUST_MCBE_SLOW_FRAME` fires on an interval over 1.1T (9.17 ms at
  120 Hz, 18.33 ms at 60 Hz) or a frame over its main CPU, render CPU or GPU budget above. T comes
  from the window's current monitor, or a slower frame cap. Text is rate-limited; every event is
  counted, marked in the frame trace and totalled as `slow_frames` in `RUST_MCBE_STAGE_PROFILE`.

## Tracy frame attribution

Tracy is the standard interactive frame/stall trace. `make play TRACY=1` enables
it; agents instead build with `--features developer-control,tracy` through `cslot`
and launch through the MCP with `headless: true`, connecting with `local_server`.
Keep INFO spans enabled. The feature is off by default; its subscriber, zones and
GPU plots are absent from ordinary builds. Domain crates remain Bevy-free.
Zone names stay fixed; changing counters and job IDs appear as zone text to avoid
exhausting the collector’s source-location table.

Install the capture tools with `brew install tracy` if missing. Match their Tracy
protocol to `tracy-client-sys` in `Cargo.lock` (the recorded tool release is in the
[evidence](../evidence/frame-breakdown-tracy.md)). With the hidden local scene settled:

```sh
tracy-capture -a 127.0.0.1 -o /private/tmp/cinnabar-frames.tracy -s 120
tracy-csvexport -u /private/tmp/cinnabar-frames.tracy > /private/tmp/cinnabar-zones.csv
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

## Native and performance evidence

Use native Bedrock/BDS comparison when it decides a contract or closes an explicit
acceptance gate, preferring version-matched, reproducible, fixed-state galleries and
exact protocol fixtures over visual guesswork. Perform live acceptance only from the
firewall-approved paths above, after integration and a build at the canonical path.
Batch equivalent captures and reuse an authoritative existing witness when it covers
the same version, state product, camera, geometry, material, and behavior question.
Performance claims require measured release evidence against the budgets below; a debug screenshot, small test scene, or green unit suite is not
performance acceptance.

For main-thread attribution, set `RUST_MCBE_STAGE_PROFILE=1` only on an
instrumented release acceptance run. The client emits one
`RUST_MCBE_STAGE_PROFILE` record per second with count, cumulative milliseconds,
and maximum milliseconds for each runtime stage; `main_frame` is the main world's
`First`..`Last` wall time, against which the chunk, actor, UI, particle, audio and
block-entity stages attribute it. Compare runs with the same
scene, BDS state, duration, release profile, and present mode. Treat overlapping
worker and main-thread stages as attribution rather than additive wall time, and
run the final performance gate again without the variable because profiling
changes the measured workload.

Normal play also emits `RUST_MCBE_SLOW_FRAME` without any environment setting,
on the trigger in Frame budgets, at most once per second with the number of
suppressed slow frames and the violated budgets. `main_ms` covers `First` to
`Last`; `between_updates_ms` covers the rest of the preceding start-to-start
interval. `main_stages` lists spans completed during that update, and
`window_stages` includes the intervening render work, both in descending
milliseconds. These are overlapping wall-time spans, not additive CPU or GPU
time. A worker span can begin in an earlier frame; its full duration appears in
the window where it completes. `surface_preparation` includes drawable
acquisition and schedule overhead; `render_submission` includes CPU render
graph execution, queue submission and presentation. A large interval with
small main work warrants checking the render stages and OS scheduling before
changing gameplay. `render_frame` is render-world time excluding drawable acquisition.

GPU timing uses timestamp queries when the adapter supports them, read back
asynchronously, so `gpu_*` stages describe a frame a few frames older than the
window they appear in. `gpu_frame` spans the first to last timestamp only when the sampled graph covers the frame.
On Metal, queries attach to existing owned render passes and the stock opaque pass:
other stock Bevy passes and shared draw categories remain unmeasured, and `gpu_frame` stays absent. Empty
compute marker passes do not produce usable timestamps on Apple GPUs. Pass categories
sum elapsed latencies, including gaps and overlapping GPU work; they are not GPU
active time and must not be added. Invalid timestamp pairs are skipped, so a
category may cover only some of its passes. Check coverage against native traces;
`RUST_MCBE_GPU_QUERY_HEALTH=1` logs per-stage invalid-pair and readback counters once a second.
Node stages
are `gpu_shadows`, `gpu_opaque`, `gpu_transparent`, `gpu_ui`, `gpu_hand`,
`gpu_post`, `gpu_tonemapping`, `gpu_fxaa` and `gpu_blit`. With
`RUST_MCBE_STAGE_PROFILE=1` on adapters with in-pass timestamps (not Apple GPUs),
draws add `gpu_terrain_solid`, `gpu_terrain_cutout`, `gpu_terrain_model`,
`gpu_terrain_depth_liquid` (direct and CPU-planned indirect draws; GPU-cull terrain
commands expose no in-pass spans), `gpu_terrain_transparent`, `gpu_actors`,
`gpu_particles`, `gpu_sky`, `gpu_panorama` and `gpu_mod_primitives`. Personal-mod post passes
add `gpu_mod_pass_0`–`gpu_mod_pass_7` by execution slot. F3 shows the latest GPU frame.

`RUST_MCBE_GPU_CATEGORIES=1` splits the main opaque phase into category passes
and disables in-pass spans to avoid double counting; separate late GPU-cull draws
are not category-timed. `RUST_MCBE_OPAQUE_LAYERS=1` measures submitted alpha-surviving
terrain coverage in a separate raster target. Both are diagnostic workloads;
keep them out of ordinary performance captures. See the
[opaque foliage fixture](../../tools/localserver/opaque-overdraw.md). Fast frames use fixed-size counters without formatting or
file I/O; aggregate snapshots and full traces remain opt-in.

After the startup visibility probe stops, ordinary world-publication logs keep
current stream and upload counters but mark the inactive visibility witness as
`visibility_snapshot_valid=false`, with null frame, pose, view and draw-mode
identities. Frozen startup visibility markers are no longer printed as current
gameplay evidence. Explicit acceptance and metrics probes retain their existing
snapshot schema.

For frame-level attribution, also set `RUST_MCBE_STAGE_PROFILE_FRAMES` to a
scratch JSON path. The profiler keeps a bounded, preallocated trace in memory
and writes Chrome trace events on the exit frame. It includes window focus and
OS occlusion at each main-frame boundary, executing threads, session geometry,
artwork and equipment setup, pack reload, world polling, and render surface
preparation. No trace file is written during normal updates. A full recording
drops subsequent events and sets `truncated`; inspect this flag before choosing
a measurement window. Surface preparation includes schedule overhead and is an
upper bound on drawable acquisition, not proof that macOS caused a stall.

Build with the `bedrock-client/frame-trace` feature for named Bevy schedules,
systems, graph nodes, command generation, queue submission and presentation spans.
Chrome tracing writes through Bevy's trace writer and changes the workload; use a
short diagnostic run, separate from the matched long captures.

`RUST_MCBE_STAGE_PROFILE_EVENTS` sets the bounded event budget (default 131,072;
maximum 2,097,152). Invalid values retain the default. Exported traces include the
capacity, exact dropped-event count, a wall-clock anchor and game-clock frame
anchors. GPU counter events carry a readback sequence and duration in nanoseconds;
their timestamp is receipt time, not GPU execution time or a CPU-frame identity.

Keep native profiler bundles outside git. Export only explicitly selected timing
and stack tables; never export the trace table of contents or process metadata,
which can include environment variables. Use scheduler states and sampled stacks
to separate blocked time from runnable delay and active work. Profiling overhead
and unavailable GPU categories must be reported separately.
