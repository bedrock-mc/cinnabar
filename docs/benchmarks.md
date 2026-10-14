# Headless chunk benchmarks

Criterion covers palette/column decode, full light solves, cube meshing, biome records,
bounded streaming bursts, dispatch-input capture, settled polling and metadata-only cohort
scans, and distant terrain sample jobs, writers and mesh jobs. Fixtures are synthetic and
require no carriers, server, window or GPU.

```sh
cargo bench --locked -p world -p meshing -p chunk-pipeline --features chunk-pipeline/benchmark-support --bench chunk_costs -- --test
cargo bench --locked -p world -p meshing -p chunk-pipeline --features chunk-pipeline/benchmark-support --bench chunk_costs -- --save-baseline chunks
```

Use the repository-pinned toolchain and the shared build-slot limiter described in
[the workflow](docs/agents/multi-agent-workflow.md). Run measurements without competing
builds or gameplay. Append a name filter, such as `stored_sections/871`, to select a
workload; subsequent runs can use `--baseline chunks` for comparison. Results stay
under the ignored `target/criterion/`.

The 871 fixture means **stored sections**, packed into 218 columns, not 871 columns.
Streaming setup preloads an implicit-air boundary through ordinary ingress, excluding
absent-neighbour grace waits from timing. Timed work includes bounded byte submission,
decode/commit, lighting, meshing, worker waits and CPU publication acknowledgements;
stream construction, boundary setup and teardown are excluded. Polling is unpaced.
Preflight CPU-step percentiles are not game-frame percentiles. Metadata scans have
no terrain. Resident-slot and stale-work counters include boundary setup; production
logs remain enabled. Decode and meshing timings include output destruction.

`pipeline/lod_sample_job` runs one opt-in distant terrain sample job on a resident column
of 4 or 24 mixed-palette sub-chunks with solved light: input capture, the content hash of
blocks, biomes and solved-light sub-chunks, material assignment, level-0 sampling and the
surface signature. The column book's check and the writer's merge are excluded;
`section_costs` below times the merge. On an Apple M1 under Asahi Linux one job took
71 µs for 4 sub-chunks and 422 µs for 24.

`pipeline/lod_writers` hands 32×32 sampled `lod` fixture columns of 8 sub-chunks to the
distant terrain writers and times them until every merge is queued in a fresh store in a
temporary directory: merges, record encoding, store reads and the slabs handed across region
borders. Sampling, store setup and starting the writer threads are excluded, and the wait
reads the merge counter without taking the writers' inbox lock. `corner_columns` centres the
columns on the corner where four writer regions meet, the best split and so the upper bound
of the scaling; `inside_columns` keeps them inside one writer region, as for a player well
inside one, where one writer takes every column. On an Apple M1 under Asahi Linux (mimalloc,
unpinned, threads at background priority) one writer merged 507 columns a second at the
corner and four writers 1,336, 2.6 times as many; inside one region one and four writers
both merged 510.

```sh
cargo bench --locked -p chunk-pipeline --features chunk-pipeline/benchmark-support,chunk-pipeline/benchmark-mimalloc --bench chunk_costs -- pipeline/lod_writers
```

`pipeline/lod_mesh_jobs` times distant terrain mesh jobs end to end on the 256 level-0
sections of 16×16 merged fixture columns: a fresh mesher wants them all and the loop polls
as the frame does until every mesh was taken, so the time covers the snapshot reads through
the store's queue, decoding and meshing on the Lod lane and the hand-over. `idle` runs alone;
`under_ingestion` runs while four writers merge another 16×16 columns into the same store,
sharing its queue. Each run prints p50 and p99 of the time from the want until a section's
mesh was taken. On an Apple M1 under Asahi Linux (mimalloc, unpinned) a run took 11.6 ms
idle (22,000 sections a second; p50 7.2 ms, p99 22.0 ms) and 38.1 ms under ingestion (6,700
a second; p50 21.5 ms, p99 52.6 ms). Reading each snapshot's sections in one batched store
request instead of one request each took the run under ingestion from 53.8 ms (p50 34.5 ms,
p99 70.4 ms) to 38.1 ms, and idle from 12.5 ms to 11.6 ms.

```sh
cargo bench --locked -p chunk-pipeline --features chunk-pipeline/benchmark-support,chunk-pipeline/benchmark-mimalloc --bench chunk_costs -- pipeline/lod_mesh_jobs
```

`pipeline/dispatch_inputs` captures 4/16/64/871 overlapping light or mesh inputs,
including handle release but excluding worker scheduling and execution. Its
`DISPATCH_INPUTS` records count allocations on the dispatch thread. The `_reused`
light cases retain worker scratch between solves; completed output remains owned.

`flight_costs` streams procedurally generated terrain (dirt over ore-flecked stone with
sealed caves) into a settled radius-10 or radius-16 view while the camera flies along +X.
Each 240 Hz frame submits the server's position, view centre and new columns, polls, and
acknowledges meshes. The flight cases report main-thread stream time per frame, and the
preflight line prints its p50, p99 and maximum. Eviction cases time only the server
position update that retires the trailing row. Cave cases time one full
connectivity search from the surface and from a sealed pocket.

```sh
cargo bench --locked -p world -p meshing -p chunk-pipeline --features chunk-pipeline/benchmark-support --bench flight_costs
```

`section_costs` times the opt-in distant terrain (LOD) foundation on synthetic fixture
columns: level-0 sampling of one column with fallback or solved light, the local L0–L4 mip
chain for a new and an unchanged sub-chunk, a full re-mip from eight children,
section packing and record encode/decode, and the ingestion writer's merge and encoding
of one column, first alone and then beside a stored neighbour. Chain mips run without
neighbour planes; the merge reads its neighbours' planes.

```sh
cargo bench --locked -p lod --features lod/benchmark-mimalloc --bench section_costs -- --test
cargo bench --locked -p lod --features lod/benchmark-mimalloc --bench section_costs -- --save-baseline lod
```

`mesh_costs` times the distant terrain mesher on fixture terrain sampled and merged column
by column over 12×12 chunk columns as the writer stores it: the dense hills of the
`section_costs` fixture, with many sealed one-block caves, and gentler `rolling` terrain
with slopes of one block in four and rare caves. It meshes every stored level-0 and level-1
section of the dense terrain whose four horizontal neighbours are stored (64 and 1; the
level-0 set includes all-air sections above the terrain), with and without exterior
culling, and a stone and air checkerboard section, the most faces a
section can have. The percentile cases print p50 and p99 over each terrain's level-0
sections of each section's median of nine runs. On an Apple M1 under Asahi Linux, pinned to
a Firestorm core with `taskset -c 7`, a dense level-0 section took 97 µs on average with
exterior culling (2,612 quads; p50 142 µs, p99 217 µs) and 142 µs without (7,441 quads;
p50 232 µs, p99 306 µs); over the same 64 positions of the rolling terrain, mostly stone
below and air above its surface, p50 91 µs, p99 125 µs with culling (117 quads) and p50
86 µs, p99 107 µs without (258 quads). The dense fixture's sealed caves are what
culling removes. The checkerboard took 1.28 ms for 98,304 quads. Most of a rolling
section's time is fixed per section: unpacking the palette into the padded words and
class masks, the border lists and the face rows.

```sh
cargo bench --locked -p lod --features lod/benchmark-mimalloc --bench mesh_costs -- --test
cargo bench --locked -p lod --features lod/benchmark-mimalloc --bench mesh_costs -- --save-baseline lod-mesh
```

`region_store` times the distant terrain disk cache on synthetic terrain-like section
records in a temporary directory: inserting and committing a 256-record batch, a read
through an open handle and through a freshly opened file, and compacting a file whose
records were overwritten twice. Commits are not fsynced, so the page cache absorbs the
writes; cold-cache and per-platform storage numbers need hardware captures.

```sh
cargo bench --locked -p lod-store --features lod-store/benchmark-mimalloc --bench region_store -- --test
cargo bench --locked -p lod-store --features lod-store/benchmark-mimalloc --bench region_store -- --save-baseline lod-store
```

These are CPU baselines, not join-time or FPS evidence. They omit socket framing,
real server terrain, GPU preparation/uploads/draws and the rest of the Bevy frame.
Native performance acceptance still follows [live testing](docs/agents/live-testing.md).
