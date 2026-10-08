# Mesh backlog follow-up

## Controlled commit bisect

All builds ran locally through the supplied `cslot`. Each historical commit used its
own production code with the same added request/reply timing entry point. One disposable
worktree reused its own target sequentially. Saved executables were then measured three
times in interleaved order while both shared build slots were reserved, excluding other
limiter-managed compilation. These are optimized test-profile CPU replays, not release FPS.

| Revision | Commit | Initial median (s) | Teleport median (s) |
| --- | --- | ---: | ---: |
| Pre-merge | `cbd3cc84` | 1.889 | 3.013 |
| CN-11 | `f6e84438` | 1.831 | 2.950 |
| CN-12 | `7b3b0505` | 1.803 | 2.956 |
| CN-05 | `109dddc8` | 1.793 | 3.014 |
| CN-02 | `5121ff5e` | 1.845 | 2.961 |
| CN-01 | `18078dac` | 1.969 | 3.073 |
| CN-03 | `0075e9ca` | 1.993 | 2.976 |
| CN-09 | `b10995bb` | 2.226 | 3.433 |
| CN-10 | `52d44ec6` | 2.317 | 3.268 |
| CN-13 | `4ef58f1d` | 2.276 | 3.304 |

CN-01 adds a small initial-load cost while removing the unbounded commit/acceptance loop.
Its post-commit decode dispatch also falls to one job per frame when that loop spends the
deadline. An expired-slice regression isolates that starvation independently of wall timing.
CN-09 is the larger repeatable regression: it probes the full nearby region on every
eligible poll, including stationary views whose queue priorities are already current.
CN-02/03/11/12 do not show a consistent initial/teleport regression here. CN-10's byte
reservations remain enabled. Its small initial difference is accompanied by a faster
teleport and does not establish another consistent regression. CN-13 produces the exact
same client-world test executable as CN-10.
The fixture does not execute pack fingerprinting or blob-cache miss resolution; it cannot
attribute performance of those separate paths. Their changes and bounds are retained.

The earlier compilation-contended sweep is retained separately. Its apparent large
CN-10 regression did not survive the controlled comparison; it is not the causal finding.
No perf-chunks commit was reverted.

## Changes

- Decode handoff remains count-bounded even after commits spend their slice; the former
  deadline check reduced it to one job per frame under sustained ingress.
- Ingress, lighting and meshing each receive time within a shared three-millisecond
  frame allocation: half for ingress/commits, with two thirds of the remaining time
  for lighting and one third for meshing. Unused ingress time remains available to both. The former two-millisecond allocation left too little scheduling
  service during sustained commits. The allocation remains below the four-millisecond
  backlog gate. Polling does not renew ingress's expired slice. Completed light work
  cannot wait behind an unlimited stream of ready commits, and ready meshes retain service.
- Lighting, mesh and decode work use separate named Rayon queues. The worker total leaves
  CPU capacity for frame/network work on machines with enough processors. Light workers
  request a lower OS priority; startup reports a rejected priority request. Urgent and
  camera-distance job ordering remain in place. Tokio's network runtime stays separate.
- An expired mesh slice performs one readiness check, rather than scanning every blocked
  candidate until one dispatches. Deferred rounds still advance, and ready work gets service.
- Stationary views reuse their heap priorities. Direct nearby probes are used during a
  view change or unfinished bounded refresh. A nearby light member lends its priority to
  its higher column dependency instead of losing that priority when the dependency is chosen.
- Repeated block arrivals coalesce a pending mesh snapshot, including a successor behind
  a cancelled worker. They preserve its revision and queue age and allow one urgency
  promotion. A dispatched job still receives a new successor and is cancelled as before.
- Non-resident and known-empty centres do not rebuild for neighbour-only changes. Their own block-data
  changes still cancel stale geometry and queue removal or replacement. Zero-byte
  removals retain normal count service while the initial geometry preparation throttle
  is active; deadlines and publication tokens still bound them.
- Fresh columns use a bounded ordered X-range lookup for resident slots, and skip
  resident-index filtering when no old authority exists. Sparse collision
  authority is indexed by column, so completing or retiring one column no longer scans
  every authoritative slot. All-air authority and collision generations stay unchanged.
- Eviction dirties removed keys and retained dependents, avoiding work for absent neighbours.
  A fully disjoint retirement detaches light and resident indexes for background destruction; overlap
  keeps the existing per-key authority checks. Running old work retains its admission
  occupancy until its closure returns, and stale completions cannot acquire new authority.

Count limits, the 64 MiB mesh-output reservation, bounded result channels, ordered commit
fences, current-light checks and neighbour/cohort deadlines remain enabled.

## Burst reproduction

The new fixture sends 48-column bursts every four frames, receiving 4,728 sub-chunks per
view through the actual request, decode, ordered commit, light and mesh paths. It injects
three relighting waves while work is outstanding and then performs a disjoint teleport.
It records peak light admission, complete drain and frame-thread ingress plus poll time.
Synthetic server reply creation, wave injection, simulated frame sleep and presentation copies are outside
that CPU work measurement. No live server or owner asset write is involved.

A separate deterministic test occupies every light worker and requires both mesh and decode
to complete before releasing any lighting worker. The existing 4,056-slot mixed roof,
emitter and transparent-geometry fixture still checks lighting convergence. Additional
regressions cover decode and ingress fairness, expired mesh readiness work, pending arrival
coalescing, stationary-view probes, independent removal service, empty-neighbour
invalidations and sparse collision authority across column eviction. The ignored burst gate requires every measured
backlog frame to stay below four milliseconds.

The historical 8,931 plateau still comes from retained retired-world telemetry, as explained
in [the original investigation](mesh-stall.md). Neither it nor this replay proves a live
38-second worker deadlock. The new replay exercises the observed admission pressure directly.

## Vanilla and platform references

- Vanilla uses distinct Streaming, Rendering, IO and other worker pools.
  Separation is not a portable CPU reservation. Worker counts and OS priorities
  are distinct; Cinnabar's sizes and platform mappings are implementation choices.
  Exact native scheduling parity remains open.
- Dirty coordinates coalesce, brightness invalidates geometry, and builds cannot
  overlap. Current-light and stale-snapshot checks remain; partially lit geometry
  is not published.
- Empty centres have fully connected visibility and no neighbour face samples;
  their own source invalidation remains active. This older representation does
  not close the current-client gate.
- A rebuild requires the region to contain the chunk. Explicit arrivals and
  eviction still invalidate their own slots.
- Moving the subscribed region retains overlapping data.
- Read-only vanilla **1.26.50.4** pack, `texts/en_US.lang:7009-7010`, describes smooth
  lighting's brightness/shadow transitions. It supplies no scheduler, pool-size or debounce
  constant. No resource-pack asset is copied into the repository.
- [Apple QoS](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/EnergyGuide-iOS/PrioritizeWorkWithQoS.html)
  and [Windows scheduling priorities](https://learn.microsoft.com/en-us/windows/win32/procthread/scheduling-priorities)
  document the platform priority hints. They are not real-time guarantees.

## Raw evidence

- `/private/tmp/mesh-bisect.log`: first per-commit sweep with compilation contention.
- `/private/tmp/mesh-quiet.log`: three controlled request/reply runs per commit.
- `/private/tmp/mesh-latest.log`: inherited focused tests and timing smoke run.
- `/private/tmp/mesh-index-focused.log`: world and streaming regression suites.
- `/private/tmp/mesh-column-final-focused.log`: final streaming and worker-isolation tests.
- `/private/tmp/mesh-quiet-burst-before-refresh.log`: intermediate controlled burst run.

## Final offline comparison

The final comparison uses nine paired repetitions in alternating before/after order,
with both local build slots reserved. Each executable starts in a fresh process.
The old executable is the saved pre-merge `cbd3cc84` fixture; the new executable uses
this follow-up. No compiler runs inside the reservation. These are complete-drain
CPU replays, including frame pacing and the same convergence checks.

| Comparison | Initial (s) | Teleport (s) |
| --- | ---: | ---: |
| Original pre-merge measurement | 2.386 | 3.801 |
| Earlier fix, `36b73166` | 3.988 | 5.452 |
| Fresh pre-merge, nine-run mean | 2.028 | 3.288 |
| Follow-up, nine-run mean | 2.030 | 2.071 |
| Fresh pre-merge, nine-run median | 2.018 | 3.292 |
| Follow-up, nine-run median | 2.055 | 2.065 |

Initial-load means differ by two milliseconds (0.1%); ranges overlap: before
1.976–2.116 s, after 1.948–2.118 s. The median difference is 37 ms (1.8%), also
reported rather than hidden. These samples do not establish a remaining initial-load
regression. Every follow-up sample beats the original 2.386 s threshold. Teleport's
mean improves by 37%; every follow-up sample beats the original 3.801 s threshold.
All runs report zero dark and mismatched-geometry meshes. The largest regular polling
sample is 3.477 ms, with the largest per-run p99 at 3.136 ms.

Three final burst passes receive 4,728 slots per view, inject three relighting waves
and reach 144 admitted light sub-chunks. Initial drain times are 2.652, 2.698 and
2.594 s; teleport times are 2.623, 2.705 and 2.654 s. Every measured ingress-plus-poll
frame passes the strict four-millisecond assertion; the largest is **3.011 ms**.
Synthetic reply creation, wave injection and presentation copies remain outside this
CPU measurement. It does not close native release FPS or network-latency acceptance.

Raw final evidence:

- `/private/tmp/mesh-final-paired-9.log`: all nine alternating-order paired repeats.
- `/private/tmp/mesh-column-quiet.log`: three paired smoke runs and three strict burst gates.
- `/private/tmp/mesh-column-final-focused.log`: 492 streaming tests passed, 14 timing tests ignored.
- `/private/tmp/mesh-index-focused.log`: world/store and streaming tests, including sparse collision authority.

## Full local gate and UI continuity

Verification runs the supplied `scratchpad/lcheck` from this worktree after local commits.
It checks formatting, workspace clippy with warnings denied, architecture, workspace
Rust tests, and the core/local-server Go checks. Its transcript is
`/private/tmp/mesh-lcheck.log`; the script's final line records the tested commit and
real exit status, also reported in the completion message.

The real-carrier offline snapshot entry point is
`ui_runtime::presentation::forms::play_flow_snapshots::snapshot_settings_signing_in_and_progress`.
Before/after PNGs live in `/private/tmp/mesh-ui-before/` and `/private/tmp/mesh-ui-after/`.
They cover settings, signing in, and both connecting/progress states. No game-server
connection or owner `.local` write is used; no PNG enters git.
