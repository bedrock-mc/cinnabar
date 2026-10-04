# Offline mesh backlog investigation

The [follow-up bisect and fixes](mesh-stall-followup.md) supersede the performance
conclusions below. This report retains the first investigation's historical measurements.

The owner log is from a release build. Local replay measurements use the repository's
optimized development/test profile, through `cslot`, while other local builds may run.
They are regression evidence, not release FPS or network-latency acceptance.

## What the captured plateau means

At 20:35:11 UTC the log clears the hotbar, reapplies an empty server-pack UI and resets
diagnostic geometry. There are no more world-publication snapshots after that reset.
The adaptive publication diagnostic continues printing the identical previous work
record: pending 8,931, in flight 6, dispatched 1, published 1.

`PublicationController::begin_frame` advanced the frame number without clearing
`last_work`. Both writers in world publication and telemetry skip their update when
there is no stream. Session retirement clears the stream in `MenuSessionState::retire`.
Consequently the repeated samples do not establish that six workers remained stuck
for 38 seconds. The retained-record design dates to `f2a4ab247`, before perf-chunks.
Resetting the record after consuming the previous frame's pressure prevents this
false stall report while preserving pressure feedback.

`light_mesh_invalidations` increments once per accepted light value change before
dependent meshes are coalesced. Its equality with `value_changed_light_jobs` is built
into the existing counter, not a measurement of 42,238 separately queued meshes.

## Vanilla evidence

- The brightness callback visits sub-chunks intersecting the changed block's
  one-cell halo and sets their dirty flag. Coordinate lookup returns an existing
  entry before allocating.
- Rebuild start requires no build in progress. Admission checks dirty/build state
  and the radius-16 column-readiness gate. The existing-geometry path compares two
  recorded ticks plus 20 and plus 60, with a flag bypass for the former. These are
  not evidence for a universal lighting debounce or waiting for all world lighting.
- One dirty-map entry per sub-chunk combines flags; rebuilds cannot overlap.
- Worker counts and OS priorities are distinct. This does not make every Cinnabar
  background task real-time or equate queue admission with CPU reservations.
- The read-only vanilla pack at
  `bedrock-samples/v1.26.50.4/full/resource_pack/texts/en_US.lang:7010` describes
  smooth-lighting transitions. `blocks.json` provides visual definitions, not thread
  scheduling or mesh debounce rules. Pack inspection supplies no scheduling constant.

Cinnabar already requires current light for every known slot in a mesh's 3×3×3 halo.
Keep that readiness check and the stale-result check; do not publish partially lit
meshes just to drain a counter. Exact native rebuild timing remains an open parity gate.

## Worker audit

CN-05 (`109dddc8`) reduced mesh admission to half the worker count, including completed
results that are awaiting acceptance. That cannot fill a worker wave, even when every
mesh is ready. Admission now allows two waves, still capped by result-channel capacity
and the existing 64 MiB output reservation. Cancellation and eviction keep their permits
until the corresponding result is consumed or dropped.

The light-to-mesh coalescing guard introduced in `2c969a8c7` excluded any key still in
flight, including keys that already had a pending successor. Repeated changes therefore
replaced that successor's revision and queue age and appended obsolete scheduling
records. `c4c7ac7d2` also appended a scan on every urgent notification. Coalescing now
applies to the pending successor too, with at most one urgency
promotion. The first invalidation still cancels and supersedes its predecessor.

Mesh, decode and light jobs use Rayon. Lighting already admits only half the pool
during an initial backlog and a quarter otherwise (minimum two batches). A batch
contains up to 32 sub-chunks: 99 in-flight light sub-chunks does not mean 99 workers.
The running-batch counter also covers evicted work until its closure returns.
Likewise mesh admission includes completed results awaiting frame-thread acceptance;
a constant in-flight count alone cannot distinguish useful throughput from a stall.

The network pump is on a separate two-worker Tokio runtime, started by the named
`bedrock-network` thread in `app/src/runtime/network/session/start.rs`. It does not
execute light solves. Neither runtime establishes OS real-time priority; that would
require platform-specific evidence and cannot be claimed from this offline test.
The render/ingress frame uses a shared cooperative two-millisecond world budget.

## Reproduction

Compare `a067c5d2` with `cbd3cc84`, the first parent of the actual perf-chunks merge
`11c02e49`. `cbd3cc84` itself merges perf-actors; it is not the perf-chunks merge.
Both revisions run the same added large-load fixture. It creates 4,056 sub-chunks,
alternating roofs, emitters and transparent geometry, then repeats light invalidation
while solves are in flight. It uses real light workers, mesh workers and stream polls.
It fails if work does not fully drain within 60 seconds, and checks every final light
owner is current. A separate 2,048-mesh fixture measures frame-paced ready throughput.

The matched request/reply harness reports both initial load and a disjoint teleport,
checks complete drain, and compares intermediate meshes with converged geometry/light.
No live server, Mojang asset write, remote build or push is part of this investigation.

## Measurements and limits

Three sequential trials per revision, interleaved before/baseline/fixed, give these
median complete-drain times in milliseconds. Other local builds shared the machine;
these are not isolated CPU benchmarks.

| Fixture | Pre-merge `cbd3cc84` | Baseline `a067c5d2` | Fixed |
| --- | ---: | ---: | ---: |
| 4,056 slots with repeated lighting | 6,731 | 6,381 | 6,877 |
| 2,048 ready meshes, 8 ms polls | not measured | 3,521 | 929 |
| Request/reply initial load | 2,386 | 4,121 | 3,988 |
| Request/reply disjoint teleport | 3,801 | 5,239 | 5,452 |

Every large-lighting run reached zero pending and in-flight light/mesh work. Every
request/reply run reached idle with zero dark or geometry mismatches. No indefinite
stall was reproduced on either historical revision. The ready-mesh improvement is
consistent across all three trials (baseline 3,461–3,592 ms; fixed 908–932 ms).
The mixed-lighting and teleport timings do not demonstrate an improvement, and the
poll spikes remain under contention. This patch does not establish that the owner's
release frame spikes or network latency have been resolved.

Two deterministic tests fail on `a067c5d2` and pass with the fix: 12 workers receive
only six ready jobs, and 100 light notifications advance one pending successor from
revision 2 to 102. The new tests require a full worker wave and one unchanged pending
revision/queue age. They also check that this successor waits for current lighting.
The large-lighting regression requires complete drain within 60 seconds; the ready
mesh regression has a 30-second bound. An app regression clears the retired-world
work record across 38 subsequent frames.

The existing `starved_mesh_dispatch_floor_admits_exactly_the_floor_through_poll`
test timed out in its light helper on both baseline and fixed builds (one of five
isolated repetitions each). The helper waited for a completion even when a bounded
scheduler turn admitted no work. It now advances another bounded turn when no light
job is in flight, retaining its overall iteration limit and convergence assertion.

Raw local logs: `/private/tmp/mesh-stall-measurements.log`,
`/private/tmp/mesh-stall-baseline-load-red.log`, and
`/private/tmp/mesh-stall-lcheck.log`. Run the ignored timing harness explicitly with
`cargo test -p client-world mesh_stall_stream_timing -- --ignored --nocapture` through
`cslot`. The full local verification command is the owner's `scratchpad/lcheck`.
