# Task 3 deterministic world-publication benchmark report

## Scope

- Original base: `315e1db fix: enforce publication snapshot contracts`.
- GPU-review base: `969a440 test: gate full world publication path`.
- Implemented only deterministic Task 3 Steps 1-3.
- Did not run the shared BDS/live acceptance step.
- Did not edit `plan.md` or push.

## Final benchmark contract

The ignored release test
`release_full_view_publication_completes_within_two_seconds` covers:

- exactly `33 * 33 * 24 = 26,136` resident known-air subchunks;
- exactly 26,136 initial accepted/current light solves with zero stale jobs;
- a six-job accepted no-op light probe whose before/after mesh-generation maps are exactly equal;
- the exact `35 * 35 * 26 = 31,850` mesh-publication dependency union;
- 64 spaced, genuinely populated bottom-layer subchunks with non-empty meshes;
- exactly 64 current upserts and 31,786 removals;
- unique `WorldMeshChange` extraction tokens with no duplicate key publication;
- a real headless Bevy `RenderApp`, production component/resource extraction, and production `prepare_gpu_chunks` acknowledgement through `complete_with_bytes`;
- matching texture and biome-tint identities on the stream and render sides, as required by production GPU candidate validation;
- a 256-item `ChunkRenderQueue` retention and application bound;
- a bounded 125-update mathematical minimum plus at most one asynchronous tail per populated mesh;
- a positive-byte production GPU acknowledgement for every one of the 64 populated witnesses;
- exact revision acknowledgement for all 31,850 publications;
- zero pending/in-flight light jobs, mesh jobs, mesh changes, render-queue items, acknowledgement slots, stale jobs, or unacknowledged revisions;
- all 26,136 resident keys current for light and clean for their applied mesh generation;
- total measured light, meshing, extraction, bounded application, GPU preparation, and acknowledgement at or below two seconds.

Renderer/device initialization and one empty priming extraction occur before the
timed publication window, matching an already-running client. Winit and
pipelined rendering are disabled so the ignored benchmark remains headless and
single-threaded at the render-sub-app boundary. The test proves GPU upload
preparation/acknowledgement, not window presentation.

## RED evidence

At original base `315e1db`, the requested `full_view_publication` filter did not
exist; there was no honest pre-existing full-path number to report. The original
fixture-coordinate and undersized-queue RED evidence remains:

```text
publication witness fixture decodes: SubChunkIndexMismatch { expected: 0, actual: -4 }

current_subchunks=26136 accepted_light=26136 light_ms=954
publication_keys=31850 upserts=1 removals=31849 upload_updates=996
max_queue_items=256 max_queue_bytes=2560 uploaded_bytes=0
stale_light=0 stale_mesh=0 mesh_ms=2131 total_ms=3105
```

The GPU review added `uploaded_bytes > 0` before changing the harness. The
existing `MinimalPlugins` benchmark failed for the intended reason:

```text
current_subchunks=26136 initial_accepted_light=26136 accepted_noop_light=6
light_ms=885 publication_keys=31850 upserts=1 removals=31849
upload_updates=249 max_queue_items=256 max_queue_bytes=2560 uploaded_bytes=0
stale_light=0 stale_mesh=0 mesh_ms=610 total_ms=1519
a genuine non-empty publication must complete production GPU upload acknowledgement
```

The first real-`RenderApp` diagnostic run identified a first-frame resource
dependency before upload work:

```text
render::plugin::publish_graphics_runtime_metadata:
Res<VisibilityDiagnosticsInput> failed validation: Resource does not exist
```

After seeding the same disabled input that extraction supplies in a running
client, the benchmark still made no acknowledgement progress. Code inspection
then found the production cause: the stream used `light_test_assets`, while the
renderer retained diagnostic-default biome/texture identities, so
`plan_gpu_chunk_updates` correctly rejected the candidate. Seeding both render
resources from the stream produced the first one-witness GPU GREEN:

```text
uploaded_bytes=2592 light_ms=932 mesh_ms=719 total_ms=1674
```

Representativeness was then raised test-first to 64 populated witnesses. The
one-witness implementation failed exactly:

```text
left: 1
right: 64
```

With 64 witnesses, the live client's 128-item setting produced real GPU upload
but missed the time gate:

```text
upserts=64 removals=31786 upload_updates=250 uploaded_bytes=190384
light_ms=1080 mesh_ms=1006 total_ms=2112
```

The final deterministic benchmark therefore uses the queue's hard 256-item
bound while retaining the same production extraction and GPU preparation
systems. It does not claim to benchmark the live 128-item per-frame setting.

## GREEN evidence

Focused command:

```text
cargo test -p bedrock-client --release --locked full_view_publication -- --ignored --nocapture
```

Two consecutive release runs before the final verification pass:

```text
current_subchunks=26136 initial_accepted_light=26136 accepted_noop_light=6
light_ms=906 publication_keys=31850 upserts=64 removals=31786
upload_updates=130 max_queue_items=256 max_queue_bytes=166080
uploaded_bytes=190384 stale_light=0 stale_mesh=0 mesh_ms=514 total_ms=1447

current_subchunks=26136 initial_accepted_light=26136 accepted_noop_light=6
light_ms=1012 publication_keys=31850 upserts=64 removals=31786
upload_updates=129 max_queue_items=256 max_queue_bytes=166080
uploaded_bytes=190384 stale_light=0 stale_mesh=0 mesh_ms=511 total_ms=1548
```

Final fresh release verification:

```text
current_subchunks=26136 initial_accepted_light=26136 accepted_noop_light=6
light_ms=894 publication_keys=31850 upserts=64 removals=31786
upload_updates=129 max_queue_items=256 max_queue_bytes=166080
uploaded_bytes=190384 stale_light=0 stale_mesh=0 mesh_ms=516 total_ms=1433
test result: ok. 1 passed; 0 failed; finished in 1.74s
```

## Verification gates

```text
cargo fmt --all --check
Exit code: 0

cargo test -p bedrock-client --locked
Exit code: 0
Main binary: 278 passed; 0 failed; 2 ignored
Auxiliary binaries: 43 + 14 + 14 passed; 0 failed

cargo clippy -p bedrock-client --all-targets --locked -- -D warnings
Exit code: 0

cargo test -p bedrock-client --release --locked full_view_publication -- --ignored --nocapture
Exit code: 0; 1 passed; 0 failed
```

## Files

- `app/src/world_stream.rs`
- `.superpowers/sdd/world-publication-task3-deterministic-report.md`
