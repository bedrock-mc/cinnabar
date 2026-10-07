# Tracy frame attribution on M3 Pro

Tracy is implemented and the reported stall range is reproduced. Samples show
waiting during main/render handoff, but exact execution time, wait objects and
scheduling causes remain unresolved. No stall-elimination or visible-FPS
improvement is claimed. Earlier combined before/after measurements remain in
[the original evidence](frame-breakdown.md); these captures are a separate
diagnostic with different instrumentation and isolated runtime inputs.

## Implementation and capture

The opt-in `tracy` feature enables Bevy schedule, system and graph spans plus
owned streaming, job, upload, readback and submission spans. Dynamic fields are
zone text: including them in source-location names exhausted Tracy's 32K table
after 13.48 seconds. Fixed names survived complete captures with 87 locations
and over 15 million zones. Ordinary builds contain no added Tracy layer, zones
or plots; domain crates use their existing `tracing` dependency.

Bevy 0.18.1's bundled `trace_tracy` recorder calibrates GPU timestamps using
encoder timestamps and a blocking completion wait, unsupported on this Metal
path. The feature therefore uses `bevy/trace`, `bevy/debug` and `tracing-tracy`
directly. Metal pass durations appear as delayed plots, not GPU timeline zones.
See [headless capture instructions](../agents/live-testing.md#tracy-frame-attribution).
Homebrew Tracy 0.13.1 matches `tracy-client-sys` 0.28.0.

The local scene has 2,401 synthetic non-flat chunks over 784×784 blocks, 349
loaded columns, hills, caves, trees and water. Saved distance stays 10 chunks
(160 blocks), frame cap stays zero, and the launch override selects Immediate
without vsync. Each fresh world copy settles for 40 seconds before 120 seconds
of measurement. Camera [48,94,-75], yaw 5.4°, pitch 20° stays fixed; the avatar
falls independently and produces a small transient particle workload. This is
not a populated lobby, flight route or stationary-avatar replay.

Both resolutions use the same optimized binary and 78 unchanged, privately
snapshotted inputs. Earlier startup attempts with drifting shared carriers were
rejected before gameplay and excluded. No builds or analysis ran during these
captures; other workstation load, temperature and driver caches were uncontrolled.
Raw traces, numeric thread samples, carriers, worlds and images stay outside git
under `/private/tmp/cinnabar-frame-breakdown`. No process environment was captured
or exported. Exports include only whitelisted zones, frames, numeric plots and
thread counters; the sampler never requests environment or process arguments.

## Full-frame measurements

Tracy marks the post-present boundary; cleanup and completion submissions can
follow. FPS below means complete hidden render intervals per second, not display
presentations. Quantiles use linear interpolation over complete interior samples.
Nested/parallel rows overlap and must not be added. The full
[625-stage table](frame-breakdown-tracy-stages.csv) includes schedules, systems,
extraction, preparation, queueing and graph nodes.

| Pixels | Complete intervals | FPS | Frame p50 / p99 / max ms | >100 ms |
|---|---:|---:|---|---:|
| 1920×1080 | 12,329 | 102.689 | 8.444 / 34.384 / 166.063 | 13 |
| 3024×1964 | 9,043 | 75.302 | 9.094 / 57.489 / 138.767 | 7 |

| Elapsed stage | 1080p p50 / p99 / max ms | Retina p50 / p99 / max ms |
|---|---|---|
| Main schedule | 1.660 / 15.572 / 151.766 | 2.246 / 27.318 / 84.534 |
| Render-extract handoff | 6.731 / 24.958 / 151.145 | 6.700 / 39.642 / 129.886 |
| Extract schedule | 0.152 / 1.660 / 27.038 | 0.166 / 3.440 / 23.879 |
| Render schedule | 8.190 / 32.511 / 167.806 | 8.760 / 53.965 / 132.700 |
| Window/surface preparation | 5.187 / 7.387 / 41.267 | 3.918 / 22.552 / 27.034 |
| Render system | 0.943 / 4.342 / 88.578 | 1.366 / 6.238 / 53.165 |
| Graph queue submission | 0.110 / 1.146 / 38.623 | 0.177 / 1.802 / 52.162 |

Only hand, transparency and UI GPU plots were published in the measured phases.
Their p50 elapsed latencies were 2.573/6.218/5.227 ms at 1080p and
7.729/16.021/16.979 ms at Retina. These overlapping summed pass latencies are
neither GPU busy time nor positions on a GPU timeline. Receipt time cannot
identify the originating frame. Retina published 8,155 samples per category for
9,044 `render_system` zones; skipped timings remain absent. See the
[plot table](frame-breakdown-tracy-gpu.csv). Full per-pass GPU attribution remains
open; these plots cannot independently establish CPU versus GPU saturation.

## Working versus waiting

A separate Retina diagnostic adds 5-ms numeric per-thread CPU/state observations
to Tracy. It records 9,943 intervals at 82.799 FPS, p50/p99/max
9.147/48.174/186.794 ms; 13 frames are between 100–170 ms and one is longer.
This is not a before/after comparison. The sampler itself used 3.238 seconds CPU
over 122 seconds and missed 287 cadence deadlines.

Clock-anchor and query-window bounds bracket counter observations around each
zone, assuming clock offsets stay within adjacent observed anchor envelopes.
They do not bound CPU execution: public XNU updates stored thread accounting at
context switches and kernel transitions; remote queries have no established
freshness bound. A precision guard cannot correct deferred accounting. See
[Apple's counter contract](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/osfmk/kern/recount.h#L48-L55)
and [remote versus current-thread reads](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/osfmk/kern/recount.c#L594-L639).
State 1 means running **or runnable**; state 3 means waiting at that instant.
Point samples are neither scheduler history nor elapsed-time proportions.
Each recorded delta covers only the named thread, not its parallel workers.

| Longest occurrence | Thread | Elapsed ms | Enclosing recorded CPU delta ms | Interior observations |
|---|---:|---:|---:|---|
| Render-extract handoff | 19309972 (main) | 188.250 | 1.387 | 33 waiting |
| Render schedule | 19310075 (render) | 186.402 | 5.443 | 23 running/runnable, 10 waiting |
| Graph submit | 19310075 | 50.797 | 1.417 | 10 running/runnable |
| Last schedule | 19309972 | 37.323 | 0.110 | 7 waiting |
| Device poll | 19310070 | 36.021 | 0.419 | 7 running/runnable |
| Hand pose write | 19310068 | 30.135 | 0.678 | 6 running/runnable |
| UI 16-byte write | 19310069 | 30.087 | 0.636 | 6 running/runnable |
| Window preparation | 19310070 | 28.768 | 0.419 | 4 waiting |

The 186.794-ms interval overlaps the render schedule and main handoff above;
the handoff crosses its boundary. Sixty-four observed client threads have counter
samples enclosing that interval, including workers, totaling 22.106 ms of recorded
counter increments. This cannot establish total execution time: accounting can lag, short-lived threads can escape
enumeration, and other processes are unmeasured. Its largest worker child,
`prepare_direct_occlusion`, lasts 30.991 ms on thread 19310068, with three
running/runnable points.
The nested occlusion poll lasts only 0.0065 ms, so that delay is not attributable
to the poll. On render thread 19310075, `render_system` lasts 13.484 ms and
`main_graph` lasts 4.091 ms. This frame's graph submit is 0.294 ms and surface
preparation 0.101 ms. Their short
duration is below the sampler's useful resolution. See the
[same-frame children](frame-breakdown-tracy-worst-frame.csv); elapsed time
still lacks a complete execution/wait breakdown. A separate 161.645-ms interval
contains the 37.323-ms Last schedule overlapping the 36.021-ms poll on another thread.
Overlap does not establish their dependency or lock owner. No mesh, lighting or
decode worker span overlaps this settled phase. The small recorded CPU deltas
and waiting observations are consistent with scheduling/wait delays, but cannot
quantify non-CPU time or distinguish yielding, contention and preemption.
Earlier native stacks show drawable waits,
allocation locks and resource retirement; they are not proof of the exact cause
of these new occurrences. See [per-thread observations](frame-breakdown-tracy-cpu.csv).

## Cost per recorded item and fix

Actual [payload counts](frame-breakdown-tracy-uploads.csv) show three pose writes
per rendered pose (132+816+816 bytes), and one 16-byte UI viewport write. At
1080p these calls have p50/p99/max 0.008/0.640/47.727 ms for pose and
0.012/2.282/20.278 ms for viewport; Retina p99s are about 10 ms despite the tiny
payloads. Particle upload calls contain 1–6 instances at 80 bytes each: 20,852
instances across 9,355 calls at 1080p, and 11,227 across 6,235 calls at Retina.
These are elapsed API costs including delays, not GPU throughput. No stationary
terrain, actor or UI geometry/texture upload was recorded. Loaded columns,
revision IDs and dispatch-wrapper calls are not processed-item denominators.

Capture exposed a separate ingress failure: a packet can publish multiple retained
consumer deltas, invalidating admission headroom sampled before an eager batch
drain. The receiver now rechecks headroom per packet and leaves blocked successors
queued, preserving FIFO, frame cap and transfer barriers. This also removes the
per-frame drain Vec. A real fan-out regression fails before and passes after;
focused tests cover backpressure/resumption and zero steady-state allocations.
Final captures complete without that disconnect. This fix is not presented as a
cause or cure of the render stalls.

## Verification and open gates

Feature-on and feature-off touched-crate checks pass through `cslot` for
`bedrock-client`, `render` and `chunk-pipeline`; the optimized headless build and
six directly affected ingress tests pass. No Tracy dependency is present in the
ordinary client dependency graph. Both final image dimensions and scene contents
were checked; four MCP game-clock comparisons pass per capture. Fixed location
cardinality is verified by the actual full captures, not a timing assertion in CI.
The optional-feature CI command includes Tracy. CI/review remain the landing gate.

Exact CPU execution and OS wait ownership, full GPU pass zones, profiler overhead,
controlled per-fix comparisons and the owner's >200 visible-FPS objective remain open.
The earlier performance branch is not fully superseded; keep its unvalidated
pooling, retention and completion ideas separate from these measured changes.

Final client SHA256:
`460e4857e2cf570b017db4550c4d40d69e2f246dcbb3633c7b9fbb8d0549989f`.
