# Headless chunk benchmarks

Criterion covers palette/column decode, full light solves, cube meshing, biome records,
bounded streaming bursts, dispatch-input capture, settled polling and metadata-only cohort
scans. Fixtures are synthetic and require no carriers, server, window or GPU.

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
chain for a new and an unchanged sub-chunk, a full re-mip from eight children, and
section packing and record encode/decode. Mips run without neighbour planes.

```sh
cargo bench --locked -p lod --features lod/benchmark-mimalloc --bench section_costs -- --test
cargo bench --locked -p lod --features lod/benchmark-mimalloc --bench section_costs -- --save-baseline lod
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
