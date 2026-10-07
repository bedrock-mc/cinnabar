# Recurring render uploads and native stall attribution

The retained upload pool removes fresh GPU staging buffers from recurring hand,
UI viewport and particle writes. It does not establish that all long stalls are
fixed. The performance gate remains incomplete.

## Implementation and deterministic checks

`Queue::write_buffer` in the pinned backend creates a staging buffer for each
nonempty call. On Metal, buffer creation takes the shared device mutex. Three
real Metal regressions observed three new hand buffers, one UI buffer and two
particle buffers during changed preparation. The same tests now observe zero.

The render crate retains eight 64 KiB mapped source buffers and space for 128
copy records. Its graph prefix adds copies before all camera subgraphs, using
the existing frame submission. Completion callbacks make a slot reusable;
pending or failed slots are never overwritten. The pool never waits for GPU
completion or grows to meet temporary pressure.

Exhaustion retains the ordinary queue path and counts fallback writes and bytes.
An overlapping fallback submits older staged writes first; the same mutex covers
that submission and the fallback to preserve issue order. Fixed Tracy zones
cover encoding and the full fallback path. Hand uniform change suppression is
preserved. This is a GPU staging allocation guarantee, not a claim that the
entire frame is free of CPU allocations, mutex contention or backend retirement.

Thirteen directly affected tests pass: three Metal allocation regressions, nine
pool lifecycle and destination-byte ordering tests, and the existing hand
uniform suppression test. The touched render crate and all its test targets
compile with Tracy. Missing Metal hardware produces a named missing-fixture
message. CI and review remain pending until the pull request runs.

## Measurement contract

Baseline is `316a85eab` from `origin/dev`; the candidate implementation is
`bf9823fd8`, changing the upload owners above. Both executables use the optimized
play profile and developer-control plus Tracy. The machine is an Apple M3 Pro with 36 GiB RAM, macOS 26.5.1, on AC power.
The system reported no recorded thermal or performance warning; that is not a
continuous temperature measurement.

Each hidden client starts through Cinnabar MCP and connects only to its managed
local synthetic terrain server. The server uses the same copied world, creative
mode and peaceful difficulty. Distance is ten chunks, vsync and frame cap are
off. The fixed camera is at `[48,94,-75]`, yaw 5.4, pitch 20, with the hand shown;
the avatar follows the same initial teleport/input sequence and may still fall
while the camera is fixed. Plain-Tracy pairs use player yaw/pitch 20/15; native
pairs use 0/0. Each before/after comparison matches its own orientation. Every measured boundary requires 349 loaded columns.

Three 120-second runs per build and resolution follow 40 seconds of settling.
Build order alternates within resolution pairs. No builds, native recordings or
large trace analysis from this task ran during this batch. Other workstation
activity was not fully controlled. Retina here means a 3024×1964 physical render target at MCP's
scale 1; it is not a separate DPI or visible-window acceptance test.

Tracy Frame intervals are post-present boundaries, not measured display scanout.
Only complete intervals wholly inside the conservative clock-mapped phase are
included. Percentiles use nearest rank; counts use strict >12.5, >33 and >100 ms.
Raw captures, images, runtime carriers and exported data stay outside Git.

| Render size | Build | Frames | p50 ms | p99 ms | Max ms | >12.5 ms | >33 ms | >100 ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1920×1080 | before | 42,905 | 8.324 | 10.126 | 149.526 | 187 | 22 | 2 |
| 1920×1080 | after | 42,909 | 8.284 | 10.567 | 76.267 | 218 | 10 | 0 |
| 3024×1964 | before | 40,516 | 8.309 | 25.368 | 149.941 | 1,472 | 22 | 2 |
| 3024×1964 | after | 41,429 | 8.322 | 25.030 | 92.051 | 999 | 27 | 0 |

| Size | Repeat | Before p99 / max ms | After p99 / max ms |
| --- | ---: | ---: | ---: |
| 1920×1080 | 1 | 9.729 / 149.526 | 9.623 / 37.187 |
| 1920×1080 | 2 | 10.069 / 72.464 | 11.606 / 76.267 |
| 1920×1080 | 3 | 10.931 / 122.281 | 10.780 / 42.301 |
| 3024×1964 | 1 | 25.559 / 149.941 | 25.126 / 89.760 |
| 3024×1964 | 2 | 25.292 / 103.572 | 25.167 / 74.634 |
| 3024×1964 | 3 | 25.275 / 97.652 | 24.054 / 92.051 |

The pooled comparison has lower maxima and no >100 ms candidate frames, but it
is not a consistent p99 improvement: 1080p p99 and >12.5 ms counts increased,
and Retina >33 ms counts increased. All 24 run-boundary snapshots had four or five
other compiler processes (aggregate CPU snapshots 0–182.4%); no other client
was present in those snapshots. These snapshots do not establish continuous
workload isolation. No causal performance claim rests on this batch alone.

Across 84,338 candidate frames, the selected writes used retained staging with
zero fallback events. Maximum observed owner spans were 0.652 ms for hand pose,
0.634 ms for UI viewport, 0.197 ms for particle instances and 0.694 ms for upload
encoding. An earlier four-slot trial exhausted its pool on Retina, so the final
pool has eight slots. Larger writes and delayed completions can still fall back.

A separate unpaired 60-second candidate Retina preflight reached 180.459 ms,
with two >100 ms frames and zero upload fallbacks. That run is excluded from the
paired table but proves that the change has not eliminated the long-stall class.


## Native evidence

All native runs attach to an existing MCP-launched PID. No launch metadata or
TOC environment section is exported. Selected files are scanned for environment
keys before analysis; stack projections omit binary/device metadata and syscall
argument values. Numeric clock anchors retain conversion uncertainty.

The initial full Metal and later 60-second clock-bearing attempts created roughly
30 GiB bundles and failed to finalize in time. Their native data is unusable and
excluded. A successful Time Profiler plus Thread State Trace run lacked a native
numeric epoch, so its running samples establish observed call paths but cannot
assign a specific Tracy stall. It observed allocation, drawable, mutex-wait,
query-reset and retirement paths across render workers. The render coordinator's
own condition-variable waits are not a substitute for tracing the workers that
execute its systems.

### Baseline Retina attribution

The attached custom Numeric Clock + Waiting Thread Samples + Thread State Trace recording used no launch environment or Metal Application stream. Only guarded numeric clock, scheduler/sample timing and projected function-stack tables were analyzed. No syscall table was analyzed. Native relative time was joined to Tracy through the numeric Unix calibration row and bracketing clock-anchor envelopes; no extrapolation was used. Maximum observed anchor-window width was 0.570 ms; individual stage bounds below use their local envelope. Absolute clock calibration error was not independently measured.

The stationary Tracy window contained 2,060 frames: p99 29.140 ms, maximum 84.382 ms; 224/11/0 frames exceeded 12.5/33/100 ms. Native coverage and bracketing anchors covered 1,642 complete frames, with p99 29.598 ms and 188/10/0 threshold counts. These are instrumented diagnostic measurements, not the plain-Tracy performance comparison.

Counts below describe non-exclusive sampled-path presence among the 188 native-complete slow frames. They are not exclusive causal stall counts. All 6,774 unique samples in this subset resolved to a scheduler interval; one boundary sample attached to two possible frames. Scheduler states, rather than sampler labels, are used. Samples do not measure a wait's duration or identify a lock owner.

| Stage or thread | Native mechanism and evidence | Quantified observation | Remaining uncertainty |
| --- | --- | --- | --- |
| `prepare_windows`, compute workers | `CAMetalLayer nextDrawable` → `CAMetalLayerPrivateNextDrawableLocked` → `usleep` / `__semwait_signal` | 351 samples in 110 slow frames. All 77 selected `prepare_windows` invocations exceeding 12.5 ms had this path. Longest: 23.438 ms, with 22.987–23.000 ms Blocked. | Drawable acquisition waits are confirmed; GPU exhaustion versus compositor/present pacing is not distinguished. |
| `hand.pose_upload`, `ui.viewport_write`, compute workers/main | `Queue::write_buffer` → `StagingBuffer::new` → Metal `create_buffer` → `parking_lot::RawMutex::lock_slow` → `cthread_yield` / `swtch_pri` | Shared-device mutex path: 60 samples in 38 slow frames. Hand stage: 10.104 ms, including 9.963–9.976 ms Preempted; UI write: 10.906 ms, including 10.778–10.788 ms Preempted. Scheduler notes explicitly say yielding. | Pinned wgpu-hal 27.0.4 identifies `self.shared.device.lock()` before `new_buffer`; this is wgpu's shared device mutex, not a proven lock inside `MTLDevice`. Owner unknown. |
| `gpu.timestamps.queue_submit`, worker/main | `Queue::submit` → `RawRwLock::wait_for_readers` / `lock_exclusive_slow` → `swtch_pri` | Three samples in three slow frames. A 10.206 ms submit contained 9.901–9.911 ms Preempted and 0.056–0.066 ms Blocked. | This is a CPU synchronization/yield path. It is not evidence of waiting for GPU completion; lock owner unknown. |
| Metal notification/completion thread | `layer_private_present_impl` → `_os_unfair_lock_lock_slow` / `__ulock_wait2` | 149 samples in 77 slow frames. | Driver presentation lock is distinct from wgpu's device mutex; no demonstrated owner or critical-path dependency. |
| Resource retirement and query bookkeeping | `MTLResourceList releaseAllObjectsAndReset` / command-buffer storage deallocation; `QueryResetMap` / `reset_queries` | Retirement: 22 samples in 22 slow frames, predominantly completion-thread work; query bookkeeping: four samples in four slow frames. | Sampled work, not proof of long frame-path blocking. Generic upload-view destructors are excluded from retirement classification. |
| Render coordinator and worker task pools | `parking::Inner::park` / `_pthread_cond_wait`; separate Runnable and Preempted scheduler intervals | Longest 84.382 ms frame: render Running 2.893 ms, Blocked 33.941–33.953 ms, Runnable 34.881–34.893 ms, Preempted 12.604 ms. Main Blocked 82.461–82.473 ms. | Long blocked intervals often have no stack snapshot. Wake dependencies and all task-pool wait owners are not established. |
| GPU completion waits, shader/pipeline compilation, file I/O | No matching completion-wait, compiler, or file-I/O stack signatures in this subset | No positive attribution | Absence of statistical samples does not prove these never occur. |

The next two frames lasted 81.415 and 79.053 ms; render Running time remained 2.371 and 2.996 ms, while Runnable + Preempted time was approximately 44.45 and 49.72 ms. Scheduler notes identify involuntary preemption by higher-priority Codex service/renderer threads, separately from the mutex paths that yielded. Another client process also preempted the render coordinator in a different frame. This was a shared workload with native-profiler overhead; the capture does not prove App Nap or isolate production scheduling behavior.

Persistent pooled uploads directly address the observed wgpu staging-allocation mutex path. This capture also establishes a remaining drawable-wait class, so pooled uploads cannot be presented as a complete stall fix. It does not establish enough ownership or GPU/present timing to justify unmeasured present-mode, resource-retirement or query changes. Exact attribution of every long stall remains open.


### Candidate Retina attribution

The matched 20-second candidate diagnostic contained 2,213 stationary frames:
p99 25.820 ms, maximum 44.569 ms and 109/2/0 threshold counts. Native coverage
contained 2,175 complete frames, including 107/2/0 above the thresholds. These
populations differ from the baseline's native coverage; the following are
non-exclusive sampled-path counts, not normalized rates or exclusive causes.

| Observed native path | Before samples / slow frames | Candidate samples / slow frames |
| --- | ---: | ---: |
| Shared-device mutex during staging allocation | 60 / 38 | 1 / 1 |
| `nextDrawable` acquisition | 351 / 110 | 391 / 96 |
| Metal presentation unfair lock | 149 / 77 | 234 / 90 |
| wgpu submit RW-lock yielding | 3 / 3 | 0 / 0 |
| Resource retirement | 22 / 22 | 6 / 6 |
| Query bookkeeping | 4 / 4 | 4 / 4 |

Selected hand-pose and UI-viewport invocations of at least 1 ms numbered 16 and
12 before, and zero after, in their slow-frame subsets. One candidate staging
mutex caller remains unresolved. No candidate fallback occurred. This supports
removing the observed allocation path from the admitted recurring writes, not
eliminating every backend buffer allocation or mutex.

Drawable waits remain: 89 candidate `prepare_windows` calls exceeded 12.5 ms.
The longest lasted 23.867 ms, including 23.656–23.659 ms Blocked and five
`nextDrawable` sleep-path samples. The longest 44.569 ms frame had render
Blocked time of 27.878–27.887 ms and Runnable time of 13.781 ms. Its other >33 ms
frame lasted 37.012 ms with 33.987 ms coordinator Blocked time. Exact task-pool
wake dependencies remain unresolved. Neither short diagnostic captured >100 ms.

### Long-frame samples: final Retina diagnostic

Two 180-second attached captures used only Numeric Clock and Waiting Thread Samples. Selected clock and projected-stack exports passed environment guards before analysis. Bracketing Tracy anchors place all five >100 ms frames inside the observed sample timestamp spans; maximum anchor-window widths were 64.250 µs before and 79.875 µs after. Absolute clock calibration error was not independently measured. This establishes sampled-path membership, not continuous coverage. Scheduler states, wait durations, preemption and lock owners remain unknown.

| Diagnostic | Frames | >12.5 ms | >33 ms | >100 ms | p99 ms | Maximum ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Before | 11,124 | 6,714 | 164 | 3 | 36.014 | 175.266 |
| Candidate | 10,443 | 8,035 | 96 | 2 | 32.672 | 190.460 |

Run-boundary snapshots recorded client counts of 0/1/1/2 and four to five compiler processes, with recorded compiler CPU reaching 251.8%. These are shared-load diagnostic results, not an isolated performance comparison.

Every >100 ms frame contains render-coordinator samples in `TaskPool::scope_with_executor_inner` → `parking::Inner::park` → `_pthread_cond_wait`, and main-thread samples in `renderer_extract` parking. The table records additional observed paths. Milliseconds within its final column are Tracy zone overlap with the frame, never native wait duration.

| Build / frame | Frame ms | Stage, worker and sampled path |
| --- | ---: | --- |
| Before / 6543 | 145.579 | Worker 24216889: one `Queue::write_buffer` → `StagingBuffer::new` → Metal `create_buffer` → `RawMutex::lock_slow` sample. Worker 24216890: `prepare_windows` overlaps 5.016 ms, with `CAImageQueueCollect_` / image release beneath `nextDrawable`; `async_executor::State::active` mutex samples also appear on the worker and render coordinator. |
| Before / 6588 | 120.148 | Worker 24216891: `prepare_windows` overlaps 13.698 ms, with two `nextDrawable` → `usleep` / `__semwait_signal` samples. Coordinator samples also include deferred ECS command application. |
| Before / 6692 | 175.266 | Render coordinator 24216908: task-pool parking, one `async_executor::State::active` pthread-mutex path, and one query-bookkeeping signature. No sampled selected worker stage explains the whole frame. |
| Candidate / 8474 | 190.460 | Render coordinator 24281161: repeated task-pool parking and an executor mutex-unlock path. `submit_graph_commands` overlaps 5.419 ms, with one `AGXG15XFamilyBlitContext deferredEndEncoding` / `free` sample. The long task-pool dependency remains unresolved. |
| Candidate / 11727 | 148.611 | Worker 24281101: `prepare_windows` overlaps 45.095 ms, with `nextDrawable` sleep and `semaphore_timedwait_trap` paths. Worker 24281099: `gpu.timestamps.device_poll` overlaps 8.063 ms, with `LifetimeTracker::triage_submissions` → Metal command-buffer and buffer deallocation samples. |

The long captures confirm that >100 ms frames remain after pooled uploads. They locate drawable acquisition, retirement and executor synchronization paths within specific frames, but do not assign the entire stalls to those paths or establish why task-pool progress was delayed. No additional production change or complete-root-cause claim is supported by these samples alone.

The candidate had zero upload fallbacks throughout this diagnostic. Maximum hand,
UI, particle and upload-encoding spans remained below 1 ms. Time inside these selected
upload and encoding zones cannot account for either long frame. GPU/present
dependencies and other backend allocation and retirement paths remain open.

## Visual pass and limits

Fresh candidate macOS frames at both physical sizes and a fresh 1080p baseline
frame were inspected for terrain geometry, depth, colors, HUD scaling and clipping. The black
diagnostic hand is present in both builds. Text-heavy UI, a particle gallery and
focus behavior remain untested. Screenshots are not committed; no visible window
was opened.

This synthetic fixed-camera fixture is not the populated-lobby or streaming-flight
release replay. Joins, cold shader caches, other GPUs and complete displayed-frame
performance remain outside this comparison. Unexplained hitches remain open.

## Reproducing the native recording

Build the small [instrument package](../../tools/profiling/native-clock.instrpkg)
with Xcode's Sampling package linked:

```sh
xcrun instrumentbuilder tools/profiling/native-clock.instrpkg -l Sampling -o /private/tmp/native-clock.instrdst
```

Start the hidden client through Cinnabar MCP, connect its local fixture, and
attach to that returned PID. Omit `--template`: the implicit Blank template avoids
conflicting sampling rates. Run the simultaneous Tracy collector separately.

```sh
xctrace record --package /private/tmp/native-clock.instrdst --instrument 'Numeric Clock' --instrument 'Waiting Thread Samples' --instrument 'Thread State Trace' --attach PID --time-limit 20s --output /private/tmp/native.trace --no-prompt
```

Never use `--launch` or export the TOC. Select only `time-info` for numeric clock
calibration, thread-state/sample timing tables, and `time-profile` function stacks
with `xctrace export --xpath`. Scan each selected file for environment containers
and typical environment keys before reading it. Remove binary/device metadata
from stack projections; do not export syscall arguments or process metadata.
The recording's sample-provider state labels can disagree with scheduler states;
use the scheduler interval containing each sample instead.
