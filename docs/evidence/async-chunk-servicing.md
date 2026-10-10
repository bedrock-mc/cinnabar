# Between-frames world servicing

The client lends `WorldStream` to a `stream-service` thread after each frame's last schedule
and reclaims it before the next frame's first. Decode commits, light and mesh acceptance and
dispatch, and terrain eviction run there while the frame extracts and paces. Hot stream,
lighting and terrain indexes use hashbrown maps, and mesh halo and neighbour checks read each
index once. The behaviour contract is in
[the chunk-pipeline architecture](../architecture/chunk-pipeline.md#between-frames-servicing).

These are offline CPU measurements of the stream alone. They are not a live join, streaming
or frame-budget acceptance; target-hardware captures remain required.

## Baseline attribution

At `295cb40d9`, radius 12 and 120 Hz, a 128 blocks/s flight spent a mean 2.9 ms per frame
in stream calls on the frame thread, and 807 of 960 frames exceeded a quarter interval. In
sampled frame-thread time, ordered-map lookups took 27.9% (`ColumnSubChunkSet::contains`
alone 14.8%) and SipHash 19.8%. The poll was budget-bound, so per-item cost also bounded
streaming throughput. A disjoint teleport evicted the old view in one 7.2 ms frame.

## Method

A standalone harness drives the real `WorldStream` through paced client frames in the app's
per-frame order: ingress, poll, committed-queue drains, retention, upload acknowledgements,
urgent work and mesh handoff, then pacing. With the service enabled it launches and reclaims
`WorldStreamService` around pacing, and counts the reclaim wait as frame-thread time. The
terrain is deterministic and synthetic, with four opaque cube blocks. An ideal GPU
acknowledges each change two frames after handoff. Publication tokens accrue at
`PHASE2_GATE`, and every measured phase after a settle starts from a full token bucket.

2026-10-09, Windows 11 (10.0.26200), Ryzen 7 9800X3D (8 cores, 16 threads), Rust 1.93.1.
Release profile with thin LTO, one codegen unit and line tables; span-only `tracy` zones are
on in every build. Base is `295cb40d9`. "Off" is this change with
`RUST_MCBE_WORLD_SERVICE=0` behaviour, polling only on the frame thread. "On" is the
production default. Each comparison alternates fresh processes, six per side unless stated,
in ABBA order and reports the median of per-run statistics. Absolute values move between
sessions; each comparison's sides ran interleaved.

## Frame-thread stream time

Microseconds per frame (ingress, poll, handoff and reclaim):

| Scenario | Build | p50 | p99 | max | mean |
| --- | --- | ---: | ---: | ---: | ---: |
| Flight, 128 blocks/s, 8 s | base | 2,164 | 4,334 | 5,196 | 2,167 |
| | on | 347 | 699 | 1,725 | 342 |
| Join, whole view at once | base | 1,044 | 3,080 | 3,113 | 1,485 |
| | on | 289 | 674 | 978 | 305 |
| Teleport, 1,000 chunks | base | 996 | 3,109 | 4,877 | 1,350 |
| | on | 113 | 615 | 888 | 146 |

The data-structure changes alone (base against off) cut mean frame-thread time by 50–54% in
all three scenarios. Servicing (off against on) cut flight from 1,012 to 344 µs mean and
from 2,598 to 701 µs p99 over eight run pairs. It also cut join p99 from 2,743 to 632 µs and
teleport p99 from 2,623 to 666 µs.

Median runs had no frame over 1.5 intervals. In the eight flight pairs, one run without the
service and three with it had one or two such frames. In each, the frame thread woke 5–10 ms
late from its pacing sleep while its stream work stayed between 1.6 and 4.4 ms; one also
waited 3.5 ms to reclaim the stream. These are OS wake delays on a desktop with other
applications running; live captures should check whether the extra busy thread makes them
more frequent.

## Readiness

Milliseconds from the scenario start (join, teleport) or column send (flight), base against
on:

| Measure | Base | On |
| --- | ---: | ---: |
| Flight column ready, p50 / p99 | 550 / 1,259 | 415 / 1,228 |
| Join, every column meshed | 1,548 | 1,249 |
| Join, view within 64 blocks | 304 | 218 |
| Join, view within 128 blocks | 746 | 1,025 |
| Join, columns within 2, all heights | 266 | 480 |
| Teleport, every column meshed | 1,541 | 1,174 |
| Teleport, view within 64 / 128 blocks | 331 / 837 | 115 / 332 |
| Teleport, columns within 2, all heights | 415 | 124 |

"View within N blocks" uses the scheduler's rank: squared distance, quadrupled behind the
view plane. In a join, the service commits the whole view within about 65 ms instead of
about 355 ms. The mesh scheduler's ranking then orders the whole committed backlog, so the
faced view completes sooner while deep and rear sub-chunks of nearby columns rank like
columns 8–20 away and finish later. Teleport and join publish each key exactly once.

In flight, faster meshing publishes edge sub-chunks before a neighbour column arrives and
publishes them again afterwards: 6.0k republished upserts at base and 10.8k with the
service, of 16.6k keys. Those upserts and the trailing-row removals spend the same
publication tokens, so the backlog after flight drains 38% later. Teleport drains in
1.8–1.9 s in every build; it is bound by tokens for its 16.5k removals.

## Service costs

- Reclaim waits 78–155 µs at p99. The maximum is the longest service step a reclaim lands
  in: eviction (about 1.0 ms per chunk crossing in flight, 3.1 ms once per teleport) or a
  column commit (up to 0.8 ms). Eviction runs whole so it precedes the next commit.
- Service-thread polling averages 0.8–1.4 ms per frame while streaming and up to a full
  interval during a backlog.
- Pacing lateness at p99 is unchanged (308 against 315 µs). The worst frame per run woke
  0.9 ms late without the service and 1.7 ms with it (medians of eight runs).
- Ingress p99 rises from about 5 to 33–71 µs in join and teleport. Commits between frames
  free more admission capacity, so each frame submits more events; the mean stays 2–5 µs.
- Remaining frame-thread time with the service is mostly mesh dispatch: 225 of 344 µs mean
  in flight.

## Correctness checks

Regression tests failed with their fixes reverted; the others pin the new contract:

- `between_frames_service_converges_to_the_frame_polled_view` presents exactly the frame-polled
  meshes across a request-mode join and a disjoint teleport.
- `raised_service_yield_limits_a_poll_to_its_guaranteed_heavy_commit` bounds a reclaim.
- `frame_poll_leaves_chunk_data_to_a_service_with_a_full_window`,
  `chunk_data_offload_still_commits_a_ready_block_change` and the four
  `skipping_chunk_data_still_*` ordered-commit tests keep block changes, teleports, light cues
  and partial batches committing in the frame. Randomized interleavings mix passes that skip
  chunk data with full passes and match strict wire order.
- `serviced_retention_retires_dropped_requests_before_the_flush`,
  `serviced_streams_evict_at_the_next_poll` and
  `serviced_retention_evicts_before_a_later_commit` keep request flushing and eviction order
  identical to frame polling.

## Reproduction and limits

The harness and its build-slot limiter live in the maintainer scratchpad (`chunkprof/` and
`cslot.py`); they are not part of the repository. Each build generates a manifest whose path
dependencies point at a checkout and use that checkout's `Cargo.lock`, offline:

```sh
python chunkprof.py build --repo <checkout> --name work
python chunkprof.py ab --a base --b work --a-args "" --b-args "--service on" \
    --runs 6 -- flight --radius 12 --speed 128 --seconds 8
```

The synthetic terrain does far less light and mesh work than real packs. The harness has no
Bevy schedule, render world, GPU or network thread. Its frame is only stream work, so the
measured gap between launch and reclaim is nearly the whole interval. A live app gives the
service less time and leaves more frame-thread polling. Live captures through the
`world_service` and `world_service_reclaim` stages in
[the live-testing guide](../agents/live-testing.md) remain the acceptance evidence.
