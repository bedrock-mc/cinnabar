# Task 3 deterministic world-publication benchmark report

## Scope

- Base: `315e1db fix: enforce publication snapshot contracts`
- Implemented only deterministic Task 3 Steps 1–3.
- Did not run the shared BDS/live acceptance step.
- Did not edit `plan.md` or push.

## Benchmark contract

The ignored release test
`release_full_view_publication_completes_within_two_seconds` covers:

- exactly `33 * 33 * 24 = 26,136` resident known-air subchunks;
- exactly 26,136 initial accepted/current light solves with zero stale jobs;
- a non-vacuous six-job accepted no-op light probe whose before/after mesh-generation maps are exactly equal;
- the exact `35 * 35 * 26 = 31,850` mesh-publication dependency union;
- exactly one current non-empty in-cohort mesh upsert and 31,849 removals;
- unique `WorldMeshChange` extraction tokens with no duplicate key publication;
- the real 256-item `ChunkRenderQueue` retention cap and the client's 128-item bounded application budget;
- exactly `ceil(31,850 / 128) = 249` bounded queue-application updates;
- exact revision acknowledgement for all 31,850 publications;
- zero pending/in-flight light jobs, mesh jobs, mesh changes, render-queue items, acknowledgement slots, stale jobs, or unacknowledged revisions;
- all 26,136 resident keys current for light and clean for their applied mesh generation;
- total deterministic CPU completion at or below two seconds.

The non-empty witness is proven applied by a spawned non-empty
`ChunkRenderInstance` and the queue's frozen exact-generation manifest before
its stream revision is acknowledged. The deterministic `MinimalPlugins`
harness has no render sub-app/GPU, so GPU-upload bytes are honestly reported
as zero; removals use `ChunkUploadAcknowledgements` directly, while the
non-empty witness is acknowledged at its observed bounded CPU application
edge. This test does not claim GPU presentation evidence.

## RED evidence

At base `315e1db`, the requested `full_view_publication` filter did not exist;
there was no honest pre-existing full-path number to report.

The first execution after adding the test failed on the fixture contract:

```text
publication witness fixture decodes: SubChunkIndexMismatch { expected: 0, actual: -4 }
test result: FAILED. 0 passed; 1 failed; finished in 1.15s
```

After correcting the witness to the fixture's encoded Y, a deliberately
undersized 32-item application budget exposed the full-path timing failure:

```text
current_subchunks=26136 accepted_light=26136 light_ms=954
publication_keys=31850 upserts=1 removals=31849 upload_updates=996
max_queue_items=256 max_queue_bytes=2560 uploaded_bytes=0
stale_light=0 stale_mesh=0 mesh_ms=2131 total_ms=3105
completed full-view publication in 3.1059106s, above the binding two-second gate
```

The benchmark was then aligned with the client's production 128-item bounded
application budget.

## GREEN evidence

Final focused command:

```text
cargo test -p bedrock-client --release --locked full_view_publication -- --ignored --nocapture
```

Final output:

```text
current_subchunks=26136 initial_accepted_light=26136 accepted_noop_light=6
light_ms=992 publication_keys=31850 upserts=1 removals=31849
upload_updates=249 max_queue_items=256 max_queue_bytes=2560 uploaded_bytes=0
stale_light=0 stale_mesh=0 mesh_ms=700 total_ms=1723
test result: ok. 1 passed; 0 failed; finished in 1.83s
```

Additional warm release repetitions completed in 1,685 ms, 1,809 ms, and
1,515 ms total, all below the two-second gate with the same exact counts.

## Verification gates

```text
cargo fmt --all --check
Exit code: 0

cargo test -p bedrock-client --locked
Exit code: 0; 280 passed; 0 failed; ignored release benchmark not run

cargo clippy -p bedrock-client --all-targets --locked -- -D warnings
Exit code: 0
```

## Files

- `app/src/world_stream.rs`
- `.superpowers/sdd/world-publication-task3-deterministic-report.md`
