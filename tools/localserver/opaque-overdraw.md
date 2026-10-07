# Opaque foliage rendering fixture

Pass `-opaque-overdraw -game-mode creative -difficulty peaceful` to the local
server. The fixture generates fresh chunks and player state, stops random ticks,
and fixes noon with clear weather. The surface is at Y=64. All scenes fit within
five chunks of spawn `(-16, 64, 0)`.

| Scene | Camera eye position | Direction |
| --- | --- | --- |
| Dense grass, flowers and leaf blocks | `(-15.5, 65.62, 0.5)` | Straight down |
| Grass horizon | `(-15.5, 65.62, 0.5)` | Horizontal toward positive Z |
| Forest canopy | `(33.5, 75.62, 0.5)` | Downward across the canopy |
| Enclosed cave | `(-15.5, 46.62, 0.5)` | Horizontal toward negative X |
| Uniform biome grass control | `(8.5, 65.62, 0.5)` | Straight down |

The western half has eight-block plains/swamp/forest patches for biome tint
blending. The eastern half has uniform plains, including the forest's persistent
leaf canopy. The cave contains one glowstone wall block and no cutout geometry.
Use a fixed camera direction and wait for terrain publication before measuring.
Teleport the player to the scene before setting a free camera; chunk delivery
follows the player's position.

## Capture workflow

Start from a development checkout with its runtime carriers prepared. From the
repository root, build the client, MCP controller, core and fixture server. Keep
intermediate artifacts private to this worktree:

```sh
CARGO_BUILD_BUILD_DIR="$PWD/target/build" ~/coding/cinnabar-wt/_agents/cslot cargo build -p bedrock-client --features developer-control
CARGO_BUILD_BUILD_DIR="$PWD/target/build" ~/coding/cinnabar-wt/_agents/cslot cargo build -p cinnabar-mcp
go build -o target/debug/bedrock-core ./core/cmd/bedrock-core
GOWORK=off go -C tools/localserver build -o ../../target/opaque-overdraw-localserver .
python3 tools/localserver/opaque-overdraw-capture.py --label before --size 1920x1080
```

The script selects vanilla rendering and launches only through MCP with
`headless: true` and `local_server`.
It teleports the player, fixes each camera, waits 15 seconds and records 20 seconds
per scene. JSON counters and screenshots go to a fresh temporary directory printed
at startup; keep these artifacts out of git. `--out` selects another directory,
`--binary` selects a saved client build, and `--assets` overrides the client's
normal pinned carrier discovery. Keep saved executables beside `bedrock-core` in
a supported `target/debug`, `target/release` or `target/play` tree: the client
resolves its installation from its executable path. `--scene`, `--warmup` and
`--seconds` limit captures.
Use matching saved render settings for both builds; a five-chunk distance appears
as 80 blocks in the recorded chunk state. `--size` is physical pixels at scale 1;
pass the target Retina dimensions explicitly.

Run `--categories` and `--overdraw` separately from ordinary timing captures.
Layer runs omit stage counters and performance summaries, and fail if no layer
samples arrive. `headless_fps` reports main-world updates per wall second in a
hidden run. `--no-vsync` is requested; the saved frame cap still applies. This
measures headless throughput, not displayed presentation FPS. These short debug
captures establish relative workload changes, not the release hardware acceptance
gate.

`--query-health` can accompany any capture mode. It enables bounded timestamp
validity counters and saves only the explicit numeric diagnostics in each scene's
`query_health` array, including the first invalid query pair per GPU stage.
`ring_skips` counts frames without an available query slot; per-stage `attempts`
counts resolved pairs, split into valid, zero, reversed and sentinel results.
A GPU stage is unavailable when every attempted pair is invalid, not zero-cost.
`frames - ring_skips` counts frames admitted to a query slot; `readbacks` counts
completed buffers with submitted spans, possibly from earlier reporting intervals.
For each stage, `valid / attempts` measures resolved-span validity, not frame
throughput. Check this coverage and the invalid-reason counts before interpreting
pass averages. Reversed timestamp pairs have been observed on Metal and are
excluded from timing; missing samples do not establish zero GPU cost.

## Diagnostics

Use `RUST_MCBE_GPU_CATEGORIES=1` only in a separate profiling capture. It splits
adjacent opaque bins into solid, cutout, model, depth-liquid and other passes,
without reordering bins or entities. Each pass writes hardware timestamps.
Native in-pass spans are disabled to avoid counting category work twice. Early
GPU-cull bins are included; separate late GPU-cull draws are not category-timed.
Pass splitting changes tile stores and loads; Metal stage spans may overlap,
so their sum is not the whole-frame time. Ordinary captures leave this flag off.
On Metal, ordinary opaque timing is attached directly to the original unsplit
render pass, including alpha-mask and skybox work; the solid occlusion-refresh,
hand and mod post passes also carry direct timestamps. Unrelated compute markers
can overlap fragment work and do not measure render-node duration. Unsupported
Metal nodes remain unmeasured, and `gpu_frame` is omitted because this coverage is
incomplete. Other backends retain existing markers. In-pass category timestamps
remain opt-in where supported. Metal readbacks wait for an asynchronous render
completion callback before resolving counters; slots remain reserved until
mapping completes. This prevents stale fragment samples without stalling rendering.

Use `RUST_MCBE_OPAQUE_LAYERS=1` in a separate diagnostic capture. Every 120 frames,
a dedicated additive half-float target counts alpha-surviving solid, cutout and
model layers with depth disabled. Original geometry, culling, texture sampling
and alpha thresholds are retained; the normal image is unchanged. Asynchronous
`RUST_MCBE_OPAQUE_LAYERS` log records contain viewport mean, maximum and histogram,
not image data. Counts describe submitted coverage, not fragment invocations or
layers which pass the ordinary depth test. More than 2,048 layers per pixel loses
unit precision; the report flags these pixels. Do not use these frames for timing.
GPU-cull mode is rejected because its later draws live outside this phase. Direct
and CPU-planned indirect draws use the replacement pipeline from each phase item.
For direct draws, wait for the solid occlusion-refresh pass to settle before
recording a stable set of layer samples.

## Baseline render audit

| Area | Existing behavior and source |
| --- | --- |
| Draw categories | `crates/render/src/chunk/draw.rs` inserts solid, cutout, model and depth-liquid bins. Direct draws insert per visible section, so a section without solid faces can establish a cutout/model bin first. Bevy's non-mesh phase retains bin insertion order and entity hash-map order. |
| Front-to-back work | The indirect path orders solid/cutout sections by camera depth, with entity ties, through `chunk/pipeline/commands.rs::front_to_back_cube_entities`. Models and depth liquids use entity order. Direct phase entities and within-section runs are not depth sorted; solid face runs are camera-facing filtered in `chunk/pipeline/solid.rs`. |
| Existing depth pass | `chunk/gpu_cull/direct.rs` temporarily draws sorted solid colour/depth before other opaque work when its static occlusion verdict changes. There is no ordinary alpha-tested cutout depth prepass. Shadow-depth shader entries belong to Enhanced rendering. |
| Discard | `chunk.wgsl` rejects invisible material faces, samples texture alpha, then discards below 0.5 for alpha-tested materials. `model.wgsl` rejects hidden/back faces and alpha below 0.5 before tint/lighting/fog. The solid entry has no discard and uses back-face culling. Discard constrains early depth writes, but does not imply that every adapter disables all early depth rejection. |
| Fragment work | Terrain uses a material-selected texture array sample, plus a second sample for interpolated animation frames. Tint, baked AO/face lighting, world lightmap and distance fog follow. `biome_tint.wgsl` has a uniform-record shortcut; mixed records visit eight lattice points, each with up to four biome weights, storage reads and gamma conversions. Model tint inputs are already block constant; cube tint positions can vary across merged geometry. |
| Filtering | `chunk/gpu/bind_groups.rs::chunk_sampler_descriptor` uses repeat, nearest magnification, linear minification/mips and anisotropy 1. `material_shader.rs::native_leaf_sampler_descriptor` uses clamp, nearest min/mag and linear mips; native leaf views are UNORM. Sampling and mip policies must remain unchanged for pixel parity. |
| Targets | `chunk/pipeline/layouts.rs` uses the ordinary 8-bit sRGB colour target with no blending and reverse-Z Depth32Float, GreaterEqual and depth writes. HDR is an explicit variant. The game camera has MSAA off; alpha-to-coverage stays disabled. |

The candidate resolves ordinary model tint once per vertex and carries it flat;
its fragment tests raw texture alpha before RGB decoding. Ordinary cubes reuse
vertex tint only for position-independent uniform biome records. Mixed or invalid
records and swamp grass retain the existing fragment lookup. Seasonal exposure,
texture filtering, lighting, fog and Enhanced/shadow behavior remain unchanged.

Reducing block-constant model tint work avoids texture/filter changes and does not
change coverage. It can reduce fragment ALU and storage traffic on immediate-mode
integrated GPUs and tile GPUs alike. An alpha depth prepass adds geometry and alpha
sampling, changes tile scheduling, and needs separate performance and coplanar
pixel-parity evidence before adoption. Category timing and layer counts alone do
not establish its benefit.
