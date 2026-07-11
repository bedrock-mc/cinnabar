# Phase 2.6 Non-Cube Models, Water, and Flipbooks Implementation Plan

> Execute this plan task-by-task with red-green-refactor. Request an independent
> review after every task and resolve all Critical/Important findings before the
> next dependent task.

**Goal:** Eliminate diagnostic geometry for every non-air protocol-1001 block
state while adding vanilla-textured non-cube models, crossed cutouts, flipbooks,
biome-tinted animated water, and bounded opaque/blend GPU streams.

**Architecture:** Keep palette-native chunk storage and the eight-byte greedy
cube record. Generate a bijective `BREG1003` state/family catalog from pinned
PMMP, PrismarineJS, Dragonfly, and Axolotl/Valentine evidence. Compile local
Mojang textures into bounded `MCBEAS04` texture pages, templates, and animation
tables. Upload compact model/liquid references plus face-specific lighting
sidecars through the existing global-arena renderer and one shared bind group.

**Design:**
`docs/superpowers/specs/2026-07-11-phase-2-6-noncube-water-design.md`

**Constraints:**

- Never commit Mojang pack or image payloads.
- Preserve `PackedQuad == 8` bytes and palette-packed runtime storage.
- All input counts, offsets, sizes, arithmetic, and GPU resources are bounded.
- Direct and MDI paths consume identical allocation/record identities.
- A phase checkbox changes only after current-HEAD automated and live evidence.

---

## Task 1: Pin and acquire the non-Mojang data sources

**Files:**

- Create: `assets/block-data-sources.json`
- Create: `scripts/acquire-block-data.ps1`
- Create: `scripts/tests/acquire-block-data.Tests.ps1`
- Create: `THIRD_PARTY_NOTICES.md`
- Modify: `.gitignore`
- Modify: `README.md`

### 1.1 RED — source-contract tests

Add Pester-compatible tests that require:

- PMMP BedrockData tag `6.7.0+bedrock-1.26.30`, commit
  `bdb44a48fb6beffb6e9f6864f06d2232eb62b6a3`, CC0-1.0;
- PrismarineJS minecraft-data commit
  `6ec59288287e4045331eaa47ee8fb104278f6b98`, MIT;
- exact SHA-256 values for `protocol_info.json`,
  `canonical_block_states.nbt`, `block_state_meta_map.json`,
  `block_properties_table.json`, `biome_definitions.json`,
  `blockStates.json`, `blocks.json`, and `blockCollisionShapes.json`;
- exact upstream license-file SHA-256 values and a checked-in notice containing
  the applicable full PMMP CC0 and PrismarineJS/Axolotl/Dragonfly MIT notices,
  copyrights, source repositories, and pinned commits;
- ignored destinations below `.local/assets/block-data/`;
- refusal to install a file whose bytes or protocol/version metadata mismatch;
- atomic temp-to-final installation and idempotent re-runs.

Run:

```powershell
pwsh -NoProfile -File scripts/tests/acquire-block-data.Tests.ps1
```

Confirm the test fails because the manifest/script do not exist.

### 1.2 GREEN — manifest and acquisition

Implement the manifest and script. Download GitHub raw/archive inputs to an
ignored cache, verify hashes before extraction/installation, validate PMMP
`1.26.30` / protocol `1001`, and print the resolved local paths. Do not add an
automatic network fetch to application startup.

Generate/review `THIRD_PARTY_NOTICES.md` from the pinned license inputs, then
keep it checked in as the human-readable redistribution artifact. Tests compare
its required source/commit/license markers; the script never silently rewrites
it during ordinary acquisition.

### 1.3 Verify and commit

Run the focused test twice, `git diff --check`, and verify staged files contain no
image/archive payload.

Commit: `build: pin protocol 1001 block data sources`

---

## Task 2: Generate typed `BREG1003` metadata with a full source bijection

**Files:**

- Modify: `tools/registrygen/main.go`
- Modify: `tools/registrygen/main_test.go`
- Modify: `tools/registrygen/go.mod`
- Modify: `tools/registrygen/go.sum`
- Modify: `crates/assets/src/registry.rs`
- Modify: `crates/assets/tests/pack.rs`
- Regenerate: `crates/assets/data/block-registry-v1001.bin`
- Regenerate: `crates/assets/data/block-registry-v1001.sha256`

### 2.1 RED — canonical join and selector tests

Add Go tests for synthetic PMMP NBT, Prismarine states/shapes, and Dragonfly
records. Require a join by namespace-qualified name plus canonical sorted typed
state compound. Cover duplicate keys, scalar-type mismatch, unequal values,
missing records, extra records, order-only equality, and deliberate hash
collision. Require a complete bijection and exact protocol/cardinality metadata.

Add exact fixtures for:

- crossed cutout: short grass, fern, flowers, wheat growth stages;
- liquid: water and flowing water depth states;
- cuboid models: bottom/top/double slabs and all eight stair states;
- storage role: primary, liquid-additional, air;
- conservative face coverage and model-family IDs;
- Prismarine shape IDs/boxes with source confidence.

Name the focused tests `TestJoinSourcesBijection`,
`TestJoinSourcesRejectsTypedStateMismatch`, `TestSelectorCardinality`, and
`TestEncodeBREG1003Canonical`. The exporter produces a bounded `Record` carrying
`ModelFamily`, `ContributorRole`, typed `ModelState`, `FaceCoverage`, and
`CollisionSeed`; the Rust decoder exposes equivalent `RegistryRecord` fields.
The expected RED failure is missing types/fields or old `BREG1002` magic, never
a fixture-path/network error.

Run:

```powershell
Push-Location tools/registrygen
go test ./...
Pop-Location
```

Confirm the new tests fail on `BREG1002`/missing fields.

### 2.2 GREEN — exporter and Rust decoder

Add bounded canonical structures for model family, typed parameters, contributor
role, coverage facts, collision/template seed descriptors, and provenance. Keep
records canonical and reject unknown enum values and selector-span disagreement.

Update Rust decoding with equivalent bounds and exact fixture assertions. Reject
`BREG1002` once the new checked-in export lands.

### 2.3 Regenerate and prove determinism

Run the exporter twice from the pinned local source paths and compare bytes and
SHA-256. Require exactly 1,356 names and 16,913 states and a complete source
bijection.

Run:

```powershell
Push-Location tools/registrygen
go test ./...
go run . -out ../../crates/assets/data/block-registry-v1001.bin `
  -pmmp ../../.local/assets/block-data/pmmp `
  -prismarine ../../.local/assets/block-data/prismarine
Pop-Location
cargo test -p assets --test pack registry_reader -- --nocapture
```

Commit: `feat: export typed protocol 1001 block visuals`

---

## Task 3: Parse complete flipbook metadata

**Files:**

- Modify: `crates/assets/src/pack.rs`
- Modify: `crates/assets/src/error.rs`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/assets/tests/pack.rs`

### 3.1 RED — parser behavior

Using synthetic JSON only, add focused tests for `ticks_per_frame`, explicit
`frames`, `atlas_index`, `atlas_tile_variant`, `replicate`, and `blend_frames`.
Cover defaults plus zero timing/replication, negative/non-integer frame values,
duplicate selector identities, excessive frame lists, and arithmetic overflow.
Only numeric/type/global-list bounds are knowable here; physical strip-frame
range validation belongs to Task 4 after image dimensions are decoded.

Run:

```powershell
cargo test -p assets --test pack flipbook -- --nocapture
```

Confirm failures show the existing two-field parser discarding metadata.

### 3.2 GREEN — bounded pack representation

Expand `RawFlipbook` and `FlipbookSource`, preserve source ordering where
semantically meaningful, canonicalize selector identities, and return precise
`AssetError` variants. Do not compile images in this task.

### 3.3 Verify and commit

Run the focused test, all `assets` pack tests, formatting, and diff checks.

Commit: `feat: parse complete Bedrock flipbooks`

---

## Task 4: Compile physical animation frames and measure texture pages

**Files:**

- Create: `crates/assets/src/animation.rs`
- Modify: `crates/assets/src/image.rs`
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/src/error.rs`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/assets/src/bin/assetc.rs`

### 4.1 RED — frame compiler tests

Add synthetic strip fixtures covering vertical/horizontal layout, explicit frame
order, replication, atlas variants, blended frames, per-frame mip generation,
byte-identical frame deduplication, `TextureRef` page/layer encoding, same-page
and cross-page sequences, physical frame-index range, 2,048-layer rollover,
third-page rejection, and exact inventory reporting. Put these tests in
`#[cfg(test)]` modules inside `animation.rs`/`compiler.rs`, where the internal
plan is directly callable.

Run:

```powershell
cargo test -p assets --lib animation::tests::flipbook -- --nocapture
cargo test -p assets --lib animation::tests::texture_page -- --nocapture
```

### 4.2 GREEN — physical layers and inventory

Slice bounded frames, generate coverage-correct mips per frame, deduplicate by
complete mip-chain bytes, and emit animation/frame tables using page-aware
`TextureRef`. Remove a source from `source_is_deferred()` only after it compiles
successfully. Emit exact static/reachable/physical/deduplicated/page counts.

Keep this task buildable by introducing a pure
`compile_animation_plan(pack, decoded_images, limits) -> AnimationPlan` and unit
tests without changing `CompiledAssets`, `RuntimeAssets`, `compile_pack`, or any
application/render public API. Expose only a bounded read-only
`inspect_animation_inventory(...) -> AnimationInventory` used by a new `assetc
animation-inventory` command; it must not install the plan into `CompiledAssets`.
The real compiler continues deferring animated sources until Task 5 performs the
atomic schema migration.

### 4.3 Measure the real local pack

Compile the pinned ignored Mojang source and record the layer inventory in an
ignored report with:

```powershell
cargo run -p assets --bin assetc -- animation-inventory `
  --pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack `
  --source-manifest assets/vanilla-source.json `
  --max-layers-per-page 2048 `
  --max-pages 2 `
  --out .local/reports/animation-inventory.json
```

Add a synthetic CLI contract test for the full argument set and require the
report to record source-manifest SHA-256, canonical pack path identity, limits,
and deterministic report bytes. Do not change the startup blob ceiling yet.

Commit: `feat: compile paged flipbook frames`

---

## Task 5: Implement the checked `MCBEAS04` codec

**Files:**

- Create: `crates/assets/src/model.rs`
- Modify: `crates/assets/src/blob.rs`
- Modify: `crates/assets/src/runtime.rs`
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/assets/src/error.rs`
- Modify: `crates/assets/tests/blob.rs`
- Modify: `crates/assets/tests/runtime.rs`
- Modify: `crates/render/src/plugin.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `crates/render/tests/plugin.rs`
- Modify: `app/src/asset_startup.rs`
- Modify: `app/src/world_stream.rs`
- Modify: `app/src/metrics.rs`
- Modify: `app/tests/assets.rs`

### 5.1 RED — exact codec and malformed fixtures

Add byte-exact fixtures for expanded visuals/materials, model templates/quads,
animations/frame `TextureRef`s, one/two texture-page descriptors, textures, and
biomes. Reject old magic, gaps/overlaps, noncanonical ordering, unknown bits,
invalid cross-section IDs, bad page/layer refs, third pages, overflow, count/size
limits, and payload/hash mismatch.

Name the core tests `mcbeas04_exact_bytes`, `mcbeas04_rejects_overlapping_pages`,
`mcbeas04_rejects_bad_texture_ref`, `runtime_decodes_mcbeas04_tables`, and
`workspace_consumers_accept_empty_new_tables`. Public outputs are
`TextureRef(u32)`, `TexturePage`, `Animation`, `ModelTemplate`, `ModelQuad`, and
expanded `CompiledAssets`/`RuntimeAssets` tables. The expected RED failure is the
missing `MCBEAS04` API. This task updates every workspace struct literal and
metric consumer atomically; renderer behavior may ignore empty new tables until
Task 6 but the full workspace must compile at the commit.

Run:

```powershell
cargo test -p assets --test blob
cargo test -p assets --test runtime
```

### 5.2 GREEN — versioned bounded decoder

Implement `MCBEAS04`, canonical encode/decode, `TextureRef`, visual kind,
template/quad, animation, frame, and page tables. Validate the entire blob before
allocating runtime sections. Keep diagnostic slot zero and strict raw-to-dense
mapping.

### 5.3 Verify and commit

Run all asset tests, strict Clippy for `assets`, and
`cargo check --workspace --all-targets --locked` to prove every consumer migrated
in the same commit.

Commit: `feat: add bounded MCBEAS04 assets`

---

## Task 6: Add animation GPU resources and shader selection

**Files:**

- Modify: `crates/render/src/plugin.rs`
- Modify: `crates/render/src/chunk.wgsl`
- Modify: `crates/render/src/lib.rs`
- Modify: `crates/render/tests/plugin.rs`

### 6.1 RED

Add tests for one/two texture pages in one bind group, page-aware material
sampling, current/next frame selection, cross-page interpolation, wraparound,
non-blended animation, asset-revision replacement, and no per-frame texture
upload. Require shader parse/validation.

### 6.2 GREEN

Upload immutable page/material/animation/frame buffers with each asset revision,
bind a diagnostic second page for one-page assets, update a tiny animation clock,
and sample page-aware current/next frames in WGSL.

### 6.3 Verify and commit

Run render shader/plugin tests and strict Clippy.

Commit: `feat: animate paged texture layers`

---

## Task 7: Generalize queue, arenas, and presentation to multiple streams

**Files:**

- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/src/plugin.rs`
- Modify: `crates/render/src/lib.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `crates/render/tests/plugin.rs`

### 7.1 RED

Define and assert sizes for `PackedModelRef` (16 bytes),
`PackedQuadLighting` (8 bytes), and liquid records.
Test combined byte accounting, atomic all-stream allocation/retry, arena growth,
stale revision/generation rejection, expected/drawn stream masks, empty streams,
and identical direct/MDI addressing.

Name the focused tests `packed_stream_record_sizes`,
`queue_counts_every_stream_and_sidecar`, `allocation_is_atomic_across_streams`,
`presentation_waits_for_expected_stream_mask`, and
`direct_and_mdi_address_identical_streams`. `ChunkMesh` produces named cube,
model, model-lighting, liquid, and liquid-lighting vectors; `ArenaAllocation`
has an optional checked geometry range for each. Camera/view-dependent ordered
snapshots and their buffers do not exist until Task 13. The expected RED
failure is missing stream fields/types, not a changed legacy fixture count.

### 7.2 GREEN

Extend `ChunkMesh`, pending uploads, arena allocations, acknowledgements, and
render manifests. Allocate every required range for one subchunk generation
atomically and retain nearest-first capped uploads.

### 7.3 Verify and commit

Run all mesh/plugin tests and check that `PackedQuad` remains exactly eight bytes.

Commit: `refactor: prepare bounded chunk render streams`

---

## Task 8: Produce face-specific lighting sidecars

**Files:**

- Create: `crates/render/src/lighting.rs`
- Create: `crates/render/tests/lighting.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/src/lib.rs`
- Create: `crates/world/src/mesh_neighbourhood.rs`
- Modify: `crates/world/src/chunk.rs`
- Modify: `crates/world/src/lib.rs`
- Create: `crates/world/tests/chunk.rs`
- Modify: `app/src/world_stream.rs`

### 8.1 RED — exact temporary lighting contract

Task 7 already defines and size-checks `PackedQuadLighting([u16; 4])`. Add
`face_specific_ao_differs_at_shared_corner`,
`phase26_light_defaults_are_explicit`, and `template_quad_lighting_order` tests
for its producer. Each vertex uses block bits 0–3, sky bits 4–7, AO bits 8–9,
with reserved bits zero. Until Phase 2.7's flood fill lands, workers bake block
light 0, sky light 15, and face-specific AO from the existing neighbor occlusion
facts. The expected RED failure is the missing calculator/default/AO behavior,
not the already-defined record or an absent world-light field.

Also add `mesh_neighbourhood_reaches_all_26_adjacent_subchunks`,
`diagonal_change_invalidates_ao_dependents`, and
`missing_boundary_samples_use_explicit_open_fallback`. The shared lower-level
interface is `world::MeshNeighbourhood`, a center plus 26 bounded optional
subchunk references/accessors; `app::world_stream` populates it and `render`
consumes it. It never depends on an app-owned type or flattens block arrays.

Define `MeshDependencyMask { diagonal_ao, liquid }` per resident target
subchunk/generation. It is computed from target palette facts during mesh
preparation and registered in `WorldStream`. On any source-block mutation—even
an ordinary opaque cube—the stream checks nearby target masks: face dependents
are always dirtied, diagonal targets only when their registered mask requires AO
or liquid samples, and unknown/new target masks are conservatively dirtied until
registered. Add tests for mask generation replacement and stale-mask rejection.

Run:

```powershell
cargo test -p render --test lighting -- --nocapture
```

### 8.2 GREEN — worker-side producer

Implement the face/vertex AO neighbor rule and the explicit Phase 2.6 light
defaults against `world::MeshNeighbourhood`. Expand mutation invalidation for
the asset-aware registered diagonal AO dependencies and produce one record per
template or liquid quad in stable order. Phase 2.7 replaces the default light
inputs and remeshes; it does not change the record or addressing.

### 8.3 Verify and commit

Run:

```powershell
cargo test -p render --test lighting -- --nocapture
cargo test -p world --test chunk
cargo test -p bedrock-client world_stream::tests::mesh_dependency -- --nocapture
cargo clippy -p render -p world -p bedrock-client --all-targets --locked -- -D warnings
```

Also rerun Task 7's record-size assertion.

Commit: `feat: bake bounded model lighting records`

---

## Task 9: Render terrestrial crossed plants and crops

**Files:**

- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/src/plugin.rs`
- Create: `crates/render/src/model.wgsl`
- Modify: `crates/render/src/lib.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `crates/render/tests/plugin.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`

### 9.1 RED

Add exact fixtures for terrestrial grass/fern/flowers/saplings/crop stages, aliases, tint
classes, cutout, two-sided visibility, model transforms, 32-quad bound,
visibility mask, per-template-quad lighting indices, queue accounting, and
direct/MDI output parity.

### 9.2 GREEN

Compile reusable crossed templates and state-to-variant materials. Emit compact
model refs from palette facts, draw with no face culling in the cutout pipeline,
and keep cave connectivity open.

### 9.3 Deterministic gallery and commit

Extend the local gallery with all cross/crop states, record zero family
diagnostics, and add a synthetic acceptance-runner test that proves the gallery
arguments/artifact identity are recorded before running visual capture.

Seagrass and kelp are deliberately excluded until Task 10 can resolve their
simultaneous liquid contributor.

Commit: `feat: render vanilla crossed plants`

---

## Task 10: Resolve palette-native multi-layer contributors

**Files:**

- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`

### 10.1 RED

Replace first-non-air tests with fixtures for solid-only, liquid-only,
solid+water, liquid-before-solid, exact duplicate liquid, distinct-liquid
conflict, multiple-primary conflict, unsupported additional contributor, and the
16-layer bound. Require one attributable diagnostic and no incorrect real
geometry on conflicts.

Name these tests `layered_solid_and_water_are_both_resolved`,
`liquid_before_solid_is_order_independent`, `duplicate_liquid_collapses`,
`two_primary_layers_fail_closed`, `distinct_liquids_fail_closed`, and
`unsupported_additional_layer_fails_closed`. The resolver returns
`ResolvedContributors { primary: Option<_>, liquid: Option<_>, diagnostic:
Option<_> }` from palette facts plus packed indices.

### 10.2 GREEN

Resolve contributor roles from palette tables and packed indices only. Do not
allocate a flat `[4096]` block array. Exclude model/liquid contributors from the
six-face diagnostic cube path.

### 10.3 GREEN — aquatic crossed models

After the contributor tests are green, add failing then passing fixtures for
seagrass/kelp texture variants, liquid coexistence, tint/animation identity, and
zero aquatic-family diagnostics. Extend the deterministic acceptance gallery and
its runner test.

### 10.4 Verify and commit

Run mesh tests and a memory/allocation-focused benchmark or assertion.

Commit: `feat: resolve layered block contributors`

---

## Task 11: Add diagonal liquid snapshots and invalidation

**Files:**

- Modify: `crates/world/src/mesh_neighbourhood.rs`
- Modify: `crates/world/src/chunk.rs`
- Modify: `crates/world/tests/chunk.rs`
- Modify: `app/src/world_stream.rs`

### 11.1 RED

Test `world::MeshNeighbourhood`'s bounded horizontal 3x3 plus vertical liquid
sample set, edge/corner deduplication, liquid-specific diagonal dirtying,
ordinary six-neighbor cube behavior, stale neighborhood rejection, and rapid
update coalescing. Prove an opaque source mutation dirties a diagonally adjacent
target whose registered `MeshDependencyMask.liquid` is true, while skipping a
face-only target. Add a compile-time/API test proving no render code consumes the
private app `MeshSnapshot` type.

### 11.2 GREEN

Add a dedicated liquid-dependent iterator/accessor to the shared world type and
have `app::world_stream` populate it. Do not add an app→render reverse dependency,
silently widen every cube dependency, or copy entire columns.

### 11.3 Verify and commit

Run world tests plus focused client world-stream tests.

Commit: `feat: track liquid mesh dependencies`

---

## Task 12: Mesh vanilla-like water

**Files:**

- Create: `crates/render/src/liquid.rs`
- Create: `crates/render/tests/liquid.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/src/lib.rs`

### 12.1 RED

Test state-derived levels, source/falling states, four-corner heights, diagonal
influence, same-liquid side/top suppression, clipped sides, bottom faces, solid
occlusion, flow direction, still/flow animation selection, waterlogging,
biome-tint identity, and one face-specific `PackedQuadLighting` per liquid quad.

### 12.2 GREEN

Implement the pure liquid mesher against the bounded snapshot/contributor API.
Keep all calculations fixed/bounded and worker-only.

### 12.3 Verify and commit

Run liquid and mesh suites.

Commit: `feat: mesh animated biome water`

---

## Task 13: Add the bounded transparent render path

**Files:**

- Modify: `crates/render/src/plugin.rs`
- Modify: `crates/render/Cargo.toml`
- Create: `crates/render/src/liquid.wgsl`
- Modify: `crates/render/tests/plugin.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`

### 13.1 RED

Test `Transparent3d`, straight-alpha/no-depth-write state, reverse-Z testing,
equal blend-group internal suppression, opaque/blend ordering, 2,097,152-ref
ceiling, upload caps, last-complete-order retention, `ViewSortGeneration` stale
completion rejection, visibility/asset/mesh invalidation, and direct/MDI ordered
snapshot parity.

Name the focused tests `transparent_pipeline_uses_alpha_without_depth_write`,
`sort_ref_ceiling_is_enforced`, `older_view_sort_generation_is_rejected`,
`last_complete_sort_remains_bound`, and
`direct_and_mdi_share_transparent_order`. `ViewSortGeneration(u64)` is part of
each sort request/result and committed ordered snapshot. The expected RED failure
is the missing transparent pipeline/sort API. Define and assert the eight-byte
`PackedTransparentDrawRef` here; its per-view vectors and double-buffered GPU
ranges are owned exclusively by the committed ordered snapshot, never
`ChunkMesh`.

### 13.2 GREEN

Add the second immutable state variant of the chunk pipeline family, liquid
arena, double-buffered sort indirection, Rayon sort jobs, generation gating, and
transparent presentation accounting.

### 13.3 Live water gallery and commit

Capture still/flow/edge/waterlogged/biome/blend scenes at multiple angles while
moving the camera. Record sort CPU, bytes, latency, frame p99, and diagnostic
counters.

Commit: `feat: render sorted transparent water`

---

## Task 14: Generate cuboid templates for slabs and stairs

**Files:**

- Modify: `tools/registrygen/main.go`
- Modify: `tools/registrygen/main_test.go`
- Modify: `crates/assets/src/model.rs`
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`

### 14.1 RED

Test Prismarine shape ingestion/provenance, collision-versus-render confidence,
box-to-exterior-quad conversion, internal-face removal, UV orientation/wrapping,
top/bottom/double slabs, all straight stairs, and neighbor-derived inner/outer
stairs. Add exact vanilla-reference gallery expectations.
Add `slab_stair_gallery_covers_all_variants` to the acceptance-runner tests,
including top/bottom/double slabs and straight/inner/outer stairs in every
orientation.

### 14.2 GREEN

Implement deterministic cuboid template generation with at most 32 quads per
reference, compact variants, conservative neighbor culling, and face-specific
lighting sidecars. Do not label an unreviewed collision box render-authoritative.

### 14.3 Verify and commit

Run generator/assets/render tests and gallery capture.

Commit: `feat: render vanilla slabs and stairs`

---

## Task 15: Render doors and trapdoors

**Files:**

- Modify: `tools/registrygen/main.go`
- Modify: `tools/registrygen/main_test.go`
- Modify: `crates/assets/src/model.rs`
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`

### 15.1 RED

Add named `door_all_32_states`, `trapdoor_all_16_states`,
`door_hinge_and_open_transform`, `trapdoor_half_and_open_transform`,
`door_uv_halves_join`, and `door_waterlogged_contributors` tests. Use complete
orientation/half/open/hinge matrices and synthetic texture aliases. Expected RED
is an unsupported family/diagnostic visual.

### 15.2 GREEN

Generate reviewed thin-cuboid variants, exact upper/lower texture selection,
conservative culling, lighting sidecars, and waterlogged contributors. Add the
complete deterministic gallery and runner contract.

### 15.3 Verify and commit

```powershell
go -C tools/registrygen test ./...
cargo test -p assets --test compiler door -- --nocapture
cargo test -p render --test mesh door -- --nocapture
pwsh -NoProfile -File scripts/tests/acceptance.Tests.ps1
```

Commit: `feat: render vanilla doors and trapdoors`

---

## Task 16: Render panes, fences, gates, and walls

**Files:**

- Modify: `tools/registrygen/main.go`
- Modify: `tools/registrygen/main_test.go`
- Modify: `crates/assets/src/model.rs`
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`

### 16.1 RED

Add `pane_all_16_connection_masks`, `fence_all_16_connection_masks`,
`gate_orientation_open_in_wall`, `wall_post_and_arm_matrix`,
`equal_glass_group_suppresses_internal_face`, and cross-subchunk neighbor tests.
Expected RED is missing neighbor-keyed template variants.

### 16.2 GREEN

Generate bounded connection-mask variants from immutable neighbor facts, including
hard panes, waterlogging, conservative partial-model connectivity, UV orientation,
and blend/cutout classification. Extend the deterministic gallery.

### 16.3 Verify and commit

Run generator, `assets` connection-model tests, `render` connection tests, and
the acceptance-runner tests.

Commit: `feat: render connected vanilla block models`

---

## Task 17: Render static chests, signs, hanging signs, and beds

**Files:**

- Modify: `tools/registrygen/main.go`
- Modify: `tools/registrygen/main_test.go`
- Modify: `crates/assets/src/model.rs`
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`

### 17.1 RED

Add `single_chest_geometry_and_uv`, `standing_sign_all_rotations`,
`wall_sign_all_facings`, `hanging_sign_attachment_matrix`,
`bed_head_foot_orientation`, and texture-atlas UV fixtures. Prove collision
boxes alone fail the chest/sign visible-geometry expectation. Double-chest joining
and rendered sign text remain explicit block-entity deferrals.

### 17.2 GREEN

Compile reviewed static template/UV tables from the available entity/sign/chest
textures, typed state selectors, and vanilla-reference galleries. Split templates
over 32 quads into deterministic multiple refs.

### 17.3 Verify and commit

Run focused compiler/mesh/gallery tests and independently review each UV layout.

Commit: `feat: render static chest sign and bed models`

---

## Task 18: Add the deterministic global visual-coverage tool

**Files:**

- Modify: `Cargo.toml`
- Create: `tools/visualcoverage/Cargo.toml`
- Create: `tools/visualcoverage/src/main.rs`
- Create: `tools/visualcoverage/tests/coverage.rs`
- Modify: `docs/phase-2-texture-slice-report.md`

### 18.1 RED

Add `reports_every_registry_state_once`, `rejects_non_air_diagnostic_visual`,
`accepts_sourced_invisible_visual`, and `report_is_byte_deterministic` tests with
small synthetic registry/blob fixtures. The binary interface is:

```powershell
cargo run -p visualcoverage --locked -- `
  --registry crates/assets/data/block-registry-v1001.bin `
  --assets .local/assets/compiled/vanilla-v1001.mcbea `
  --out .local/reports/visual-coverage.json
```

It writes sorted name, typed state, visual kind, material/template/liquid ID,
source/provenance, and diagnostic status plus exact totals. Exit nonzero when any
non-air state remains diagnostic or any state is missing/duplicated.

### 18.2 GREEN

Implement the bounded Rust CLI using the production registry/runtime decoders.
Do not maintain a second parser. Update the phase report with the command and
artifact schema, not a generated local report payload.

### 18.3 Verify and commit

Run tool tests twice, compare report bytes, and run it against the current blob
to record the expected failing residual baseline.

Commit: `tools: report exhaustive block visual coverage`

---

## Task 19: Exhaust the residual visual families

**Files:**

- Modify: `tools/registrygen/main.go`
- Modify: `tools/registrygen/main_test.go`
- Modify: `crates/assets/src/model.rs`
- Modify: `crates/assets/src/compiler.rs`
- Modify: `crates/assets/tests/compiler.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/tests/mesh.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`
- Modify: `docs/phase-2-texture-slice-report.md`

### 19.1 RED — one named batch at a time

Drive these reviewed batches from the Task 18 report, adding exact state/UV/model
tests before each implementation:

1. surface-height cuboids: snow layers, carpets, farmland/path, pressure plates;
2. thin/flat connections: ladders, rails, redstone, vines, lichen, resin, sculk;
3. fixtures: torches, lanterns, candles, flower pots, cakes;
4. machines/decorative cuboids: hoppers, bells, anvils, grindstones, lecterns,
   brewing/enchanting tables;
5. explicit vanilla-invisible/engine-only states with sourced non-diagnostic
   reasons;
6. every remaining sorted report family, creating a named generator class and
   focused test rather than mapping it to an arbitrary cube.

Each batch's RED run is `cargo test -p assets --test compiler <batch>` plus
`cargo test -p render --test mesh <batch>` and must fail on diagnostic visuals.

### 19.2 GREEN

Implement and review each batch independently. Re-run `visualcoverage` after
every batch; the diagnostic count must decrease by the exact expected state set.

### 19.3 Verify and commit

Require 16,912 non-air states with non-diagnostic visual identities, no duplicate
or missing state, zero unsupported families, and zero live diagnostic geometry.

Commit final batch: `feat: complete protocol 1001 visual coverage`

---

## Task 20: Set measured limits and close Phase 2.6 acceptance

**Files:**

- Modify: `app/src/asset_startup.rs`
- Modify: `app/tests/assets.rs`
- Modify: `crates/assets/src/bin/assetc.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`
- Modify: `docs/phase-2-texture-slice-report.md`
- Modify: `plan.md`

### 20.1 RED — measured startup/acceptance contracts

Add tests tying the startup ceiling to a documented real `MCBEAS04` size with
headroom, adapter page/layer checks, all-stream upload bounds, global zero
diagnostics, view-sort metrics, and reproducible artifact identities.

### 20.2 GREEN — integration

Set the smallest documented bounded startup ceiling above the measured real blob.
Update `assetc` inventory output and acceptance collection. Keep the clean
no-assets path green.

### 20.3 Full automated gate

Run fresh:

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

Also run registry determinism, PowerShell acquisition/acceptance tests, and the
real local asset compile. Verify no Mojang payload is tracked.

### 20.4 Live acceptance

Launch BDS/core/client and exercise focus, keyboard movement, mouse look, and
rotation. Capture native Windows screenshots at multiple distances/angles for:

- plants/crops and cutout edges;
- animated/flowing/biome-tinted/waterlogged water;
- slabs and straight/inner/outer stairs;
- doors/trapdoors/panes/fences/gates;
- chest/sign static models and residual-family galleries;
- title-bar FPS and no diagnostic magenta.

Record combined RSS, steady CPU, p99 frame time, sort/upload metrics, and the
two-second teleport/full-view remesh gate. Phase 2.6 is checked only when every
automated and live requirement above is evidenced at current HEAD.

Commit: `docs: close phase 2.6 rendering evidence`
