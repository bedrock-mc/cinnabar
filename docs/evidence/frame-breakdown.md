# M3 Pro frame attribution (macOS 26.5.1)

These are diagnostic hidden-window captures, not release or displayed-frame
qualification. The long-stall gate remains open. The owner’s saved render distance
is 10 chunks; it was read, copied to the isolated client and left unchanged.
The later [Tracy captures](frame-breakdown-tracy.md) reproduce 100–170 ms stalls
and correlate elapsed spans with thread-state samples; they are not pooled here.

## Measured workload

Local synthetic non-flat terrain, 2,401 pregenerated chunks over 784×784 blocks,
349 loaded columns, fixed camera, clear noon, hills, caves, trees and water.
Three 120-second runs per build/resolution follow 40 seconds settling, using fresh
copies of the same template. Vsync is overridden off; Metal selects Immediate.
The fixed camera is independent of the descending local player. No populated
actor/particle workload or flight route is included. The fixture is not vanilla
terrain generation; see [fixture instructions](../../tools/localserver/terrain-fixture.md).

The optimized `play` build uses developer control and bounded stage recording.
Named CPU traces and native GPU/CPU traces are separate, shorter captures.
The final binary is recaptured after preserving hidden item preparation order.
A disconnected earlier Retina native capture is excluded. No builds or heavy
analysis run during measured intervals. Other workstation load, thermal history
and driver caches are not controlled.

| Physical pixels | Build | Updates/s | p50 / p99 / max ms | Hitches >12.5 ms |
|---|---|---:|---|---:|
| 1920×1080 | before | 110.54 | 8.339 / 27.350 / 119.978 | 2671 |
| 1920×1080 | after | 119.64 | 8.402 / 11.711 / 35.972 | 238 |
| 3024×1964 | before | 114.42 | 8.361 / 24.564 / 149.094 | 1614 |
| 3024×1964 | after | 118.22 | 8.285 / 12.596 / 87.048 | 438 |

FPS means hidden client updates per second, not display presentations. Quantiles
are nearest-rank over pooled raw samples. The runs and instrumentation vary enough
that these numbers do not prove a general speedup or elimination of the stalls.
Hardware comparisons cover the combined changes, not per-fix ablations.

## Breakdown

| Pixels | Stage | Before p50 / p99 / max ms | After p50 / p99 / max ms |
|---|---|---|---|
| 1920×1080 | main_frame | 1.258 / 9.142 / 80.965 | 1.132 / 2.250 / 15.468 |
| 1920×1080 | render_frame | 2.925 / 23.165 / 102.031 | 2.584 / 5.816 / 30.549 |
| 1920×1080 | surface_preparation | 5.108 / 7.470 / 23.025 | 5.432 / 7.587 / 24.087 |
| 1920×1080 | render_submission | 1.244 / 4.834 / 60.117 | 1.062 / 2.697 / 10.496 |
| 1920×1080 | world_stream | 0.004 / 0.014 / 0.339 | 0.002 / 0.010 / 0.444 |
| 3024×1964 | main_frame | 1.131 / 6.082 / 95.695 | 1.093 / 2.217 / 17.489 |
| 3024×1964 | render_frame | 2.914 / 14.473 / 141.188 | 1.999 / 4.890 / 78.974 |
| 3024×1964 | surface_preparation | 5.108 / 7.600 / 25.870 | 5.879 / 7.584 / 24.625 |
| 3024×1964 | render_submission | 1.257 / 4.071 / 40.762 | 0.909 / 2.276 / 33.814 |
| 3024×1964 | world_stream | 0.003 / 0.015 / 0.073 | 0.002 / 0.010 / 0.042 |

CPU spans are elapsed time, including waits and preemption; stages overlap.
`render_frame` excludes surface preparation but is not a literal contiguous span.
The named table separates extraction, handoff, systems, graph nodes, command
creation, actual queue submission and presentation. Render handoff includes waits.

- [All long-run runtime stages](frame-breakdown-runtime.csv).
- [Named CPU schedules, systems and graph nodes](frame-breakdown-named.csv).
- [Native GPU busy unions and elapsed spans](frame-breakdown-gpu.csv).

| Pixels | Native GPU active intervals | Before p50 / p99 / max ms | After p50 / p99 / max ms |
|---|---|---|---|
| 1920×1080 | All named render passes | 4.240 / 6.377 / 6.957 | 6.728 / 13.069 / 16.110 |
| 1920×1080 | Shared opaque pass | 3.613 / 5.389 / 5.790 | 5.820 / 8.896 / 10.579 |
| 3024×1964 | All named render passes | 8.167 / 10.283 / 10.837 | 11.862 / 16.501 / 17.560 |
| 3024×1964 | Shared opaque pass | 6.657 / 7.213 / 9.414 | 6.798 / 8.108 / 9.924 |

Native intervals show active vertex/fragment work; timestamp query categories
sum elapsed pass latencies, including gaps and overlapping GPU work. They are
not additive. The final Retina hand query agrees with its native elapsed span,
not its active work. UI aggregate disagreement remains unresolved because
individual valid/zero query counts were not captured. Partial Metal coverage
never reports `gpu_frame`.

Terrain, cutout and other opaque draws share a stock pass. M3 cannot timestamp
individual draws inside it without changing the workload. Entities, particles
and shadows have no populated workload here. Actual draw/item denominators were
not captured; loaded columns are not drawn items. Per-item cost remains open.

At 1080p, drawable sleep and surface preparation dominate the CPU frame path;
this does not establish a CPU-compute limit. Target GPU active-window coverage
is 55.21% before and 83.82% after. Retina is under GPU throughput pressure:
coverage is 97.89% before and 99.77% after, not whole-device utilization.
The short native GPU results regress, including the no-camera clear pass and
changed overlap. Real query instrumentation, profiler overhead, caches and
thermal history are not isolated. No GPU speedup is claimed. Controlled,
interleaved timing-on/off captures remain necessary. Owned-pass queries also run
in normal gameplay on supported adapters; their overhead is a production risk.
Fixed query/readback storage does not remove backend encoder allocations.

The final long captures still include 676 frames above 12.5 ms, with an 87.048 ms
maximum. Complete stall elimination and the owner’s FPS objective remain open.

## Attribution and changes

- Native stacks show `nextDrawable` sleeping during surface acquisition. The
  hidden Immediate surface still behaves near 120 Hz. This limits interpretation
  of visible, uncapped FPS; no >200 FPS result is claimed.
- A lean native trace finds Metal allocation-lock contention through
  `Queue::write_buffer` and fresh staging buffers. Inactive portals, empty items,
  unchanged hand uniforms and unchanged cloud colour now skip up to six writes,
  six staging allocations and 336 bytes per eligible frame. Other uploads remain.
- Query-reset bookkeeping allocates driver blit contexts. Owned render-pass
  descriptor queries replace 20 empty compute marker encoders per eligible
  graph frame. Empty markers returned zero timestamps on this M3. Query resets
  and the existing asynchronous resolve submission still have overhead.
- Matched 15-second native windows show render scheduled residency decreasing
  from 2,524.674 to 1,958.269 ms and main residency from 1,709.121 to 1,549.361 ms.
  Staging-allocation samples fall from 308 to 158, allocation-lock samples from
  91 to 45, and retirement samples from 148 to 80. Sampling is statistical;
  the mechanisms remain and these windows do not reproduce the long stall.
- Submission retirement and Metal buffer destruction occur under `Device::poll`
  on frame-path workers. This work is observed, not eliminated. The fixed
  timestamp ring skips timing when full and never waits for GPU completion.
- One heavily instrumented 95.515 ms native episode has a fully blocked main
  thread. Render spends 4.415 ms running, 38.423 blocked, 30.810 runnable and
  21.831 preempted. Memory-management and profiler preemption are visible.
  It is not exactly correlated with the separate 149.094 ms runtime maximum.
  The lean trace retains allocation/drawable/retirement stacks without reproducing
  that episode. Complete end-to-end long-stall causality remains unverified.
- No causal pipeline compile, shader-library load or application IO stack was
  found in sampled windows. Sampling absence is not proof of zero such work.

Only selected timing and symbol tables were exported. No process environments
or TOC were exported. Raw traces, worlds, binaries and images stay outside git.

## Verification

Six regression cases fail before the fixes and pass afterward. Focused render
tests pass, including hardware Metal timestamps, nonblocking readback capacity,
allocation/upload guards, retained buffer identities, reactivation and hidden
catalog preparation before visibility. Focused Go fixture tests and formatting
checks pass.

Touched-crate checks pass through the shared build limiter:
`cargo check --tests -p render --features enhanced` and
`cargo check --tests -p bedrock-client --features developer-control,frame-trace`.
The optimized client build and fresh hidden frame inspection pass at both
physical resolutions. CI/review remain the integration gate; no full-workspace
sweep or local CI ladder was run.

Baseline executable SHA256:
`ca45a9c1ac2bcbeedd9a3af58324e2c02a3204a700f598951dbe37531df6794c`.
Candidate executable SHA256:
`9e1eaa7e159c103745ca1c56a6f42acf2224a465feeec6a475250667962640a9`.
The baseline is dev `ea3b3b93c` plus only recording capacity/clocks and named tracing.

The earlier performance branch is not fully superseded. Its hand-uniform cache
idea is adapted to the current full lighting payload here; staging pooling,
bind-group retention, prewarming and completion consolidation remain separate,
unvalidated changes. Do not combine overlapping hand/timing edits wholesale.
