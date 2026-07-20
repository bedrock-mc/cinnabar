# Mesh Light Halo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Feed current palette-native client light into mesh workers through a bounded fixed 27-slot identity halo that rejects stale completions without losing pending work.

**Architecture:** The render crate defines a two-nibble `LightSampler`; the app implements it with a fixed `[Option<MeshLightSlot>; 27]` captured in `MeshSnapshot`. Dispatch gates on all known halo slots, completion acceptance revalidates exact light and private direct-sky identities, and light lifecycle changes dirty all checked center-plus-26 mesh dependants.

**Tech Stack:** Rust 2024, Arc-backed world light storage, Rayon workers, crossbeam result channels, cargo test/clippy/fmt.

## Global Constraints

- Start from exact commit `47ae126085b5ec4e6c54683fc9f130d2af2f45ea`.
- Do not create flat 4,096-entry block or light arrays.
- Render-facing samples expose block and sky nibbles only; direct-sky remains private scheduler provenance.
- Preserve `PackedQuadLighting`, GPU arena, shader, draw, and asset formats.
- Preserve existing mesh/light worker and result capacities.
- Do not modify assets or bee/block-family work.
- Keep live full-view remesh and GPU/shader Phase 2.7 acceptance open.

## Execution Scope Amendment

After this plan was committed, the render sampler and CPU sidecars were assigned
to a separate branch. Task 1 is intentionally not implemented here. This branch
implements Tasks 2-4 and the app/world/release portions of Task 5. Adapting the
internal `MeshLightHalo::sample_channels` seam to the separately owned render
sampler and CPU mesh-bake entry point remains open for branch integration.

**Integration update (2026-07-14):** Task 1 and the app adapter are now merged:
the halo implements the render sampler, the worker uses the light-aware mesher,
and cube/model/cross/liquid CPU sidecars retain the solved channels through the
bounded render queue. GPU arena/shader consumption remains open.

---

### Task 1: Allocation-free render light sampler

**Files:**
- Modify: `crates/render/src/lighting.rs`
- Modify: `crates/render/src/lib.rs`
- Modify: `crates/render/src/mesh.rs`
- Modify: `crates/render/tests/lighting.rs`

**Interfaces:**
- Consumes: signed center-relative mesh sample coordinates and existing `PackedQuadLighting`.
- Produces: `LightSample::new(block, sky)`, `LightSample::dark()`, `LightSampler::sample_light([i32; 3])`, and a light-aware mesh entry point while preserving the old constant-light wrapper for unaffected callers.

- [ ] **Step 1: Write failing sampler and baking tests**

Add tests with a coordinate-recording sampler proving the lighting helper requests the exact face/corner coordinates and packs the returned block/sky nibbles while AO remains unchanged. Add invalid-nibble construction tests and verify no direct-sky field exists in the public sample.

```rust
struct CoordinateLight;
impl LightSampler for CoordinateLight {
    fn sample_light(&self, coordinate: [i32; 3]) -> LightSample {
        if coordinate == [16, 16, 16] {
            LightSample::new(7, 11).unwrap()
        } else {
            LightSample::dark()
        }
    }
}
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p render --test lighting light_sampler --locked -- --nocapture`

Expected: compilation fails because `LightSampler` and `LightSample` do not exist.

- [ ] **Step 3: Add the minimal render contract**

Define:

```rust
pub trait LightSampler: Sync {
    fn sample_light(&self, coordinate: [i32; 3]) -> LightSample;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LightSample { block: u8, sky: u8 }
```

Validate both channels are `<= 15`, expose const getters/dark fallback, pass `&impl LightSampler` through lighting helpers, and add a light-aware neighbourhood mesh function. Keep the existing public mesh wrapper using a private Phase 2.6 constant sampler so unrelated callers and formats remain stable.

- [ ] **Step 4: Run GREEN and render regression tests**

Run: `cargo test -p render --locked`

Expected: all render tests pass and the new sampler tests observe exact packed block/sky values.

### Task 2: Fixed 27-slot palette-native halo sampler

**Files:**
- Modify: `app/src/world_stream.rs`

**Interfaces:**
- Consumes: `LightStore`, `LightOwnership`, `StoredDirectSky`, and `render::LightSampler`.
- Produces: `MeshLightSlot`, `MeshLightHalo`, `MeshLightHalo::sample_light`, exact canonical slot indexing, and `MeshSnapshot.light_halo`.

- [ ] **Step 1: Write failing center/face/edge/corner and fallback tests**

Build 27 uniform `Arc<SubChunkLight>` values keyed by their offset and assert coordinates route as follows:

```rust
assert_eq!(halo.sample_light([0, 0, 0]), center);
assert_eq!(halo.sample_light([16, 4, 4]), positive_x_face);
assert_eq!(halo.sample_light([16, 16, 4]), positive_x_y_edge);
assert_eq!(halo.sample_light([-1, -1, -1]), negative_corner);
assert_eq!(halo.sample_light([32, 0, 0]), LightSample::dark());
```

Also prove `[-1, -1, -1]` maps to local `[15, 15, 15]`, an absent in-range slot is dark, and constructing/sampling the halo does not allocate a 4,096-entry buffer.

- [ ] **Step 2: Run RED**

Run: `cargo test -p bedrock-client mesh_light_halo_samples --locked -- --nocapture`

Expected: compilation fails because `MeshLightHalo` does not exist.

- [ ] **Step 3: Implement the fixed halo and attach it to snapshots**

Use:

```rust
struct MeshLightSlot {
    key: SubChunkKey,
    block_generation: u64,
    light_revision: u64,
    light: Arc<SubChunkLight>,
    direct_sky: Arc<DirectSkyMask>,
}

#[derive(Default)]
struct MeshLightHalo {
    slots: [Option<MeshLightSlot>; 27],
}
```

Route each axis with `div_euclid(16)` and `rem_euclid(16)`, reject offsets outside `-1..=1`, and read `LightChannel::Block/Sky` directly from `SubChunkLight`. Clone only Arc handles when creating a mesh snapshot.

- [ ] **Step 4: Run GREEN**

Run: `cargo test -p bedrock-client mesh_light_halo --locked`

Expected: center, face, edge, corner, and fallback tests pass.

### Task 3: Dispatch gating and exact stale completion rejection

**Files:**
- Modify: `app/src/world_stream.rs`

**Interfaces:**
- Consumes: `MeshLightHalo`, `WorldStream::light_is_current`, live ownership/direct-sky state.
- Produces: `mesh_light_halo(key) -> Option<MeshLightHalo>`, `mesh_light_halo_is_current(&MeshLightHalo) -> bool`, completion-carried identities, and lossless mesh requeue.

- [ ] **Step 1: Write failing ordering and gating tests**

Cover:

```rust
// mesh-first: pending remains while one known face/edge/corner light is dirty
assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
assert!(stream.pending_mesh.contains_key(&center));

// light-first: after every known slot is current, exactly one job dispatches
assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
```

Use all three non-center dependency shapes so dispatch cannot accidentally gate only faces or center.

- [ ] **Step 2: Run RED**

Run: `cargo test -p bedrock-client mesh_dispatch_waits_for_current_light_halo --locked -- --nocapture`

Expected: the existing center-only gate dispatches with a dirty non-center slot.

- [ ] **Step 3: Gate dispatch and carry captured identities**

Make `mesh_light_halo` return `None` when any known/resident slot is not current. Build the snapshot before removing `pending_mesh`; include the halo in `MeshCompletion`; call the light-aware render mesh entry point on the worker.

- [ ] **Step 4: Run ordering GREEN**

Run: `cargo test -p bedrock-client mesh_dispatch_waits_for_current_light_halo --locked`

Expected: both completion-order tests pass.

- [ ] **Step 5: Write failing mid-flight replacement/no-loss tests**

Capture and dispatch a mesh, replace a face/edge/corner light Arc at the same mesh revision, then accept the old completion. Assert no upload is published, `stale_mesh_jobs` increments once, `in_flight` is cleared, and the same current revision is present in `pending_mesh`. Also assert a newer mesh revision is retained instead of overwritten.

- [ ] **Step 6: Run RED**

Run: `cargo test -p bedrock-client stale_light_halo_mesh_completion --locked -- --nocapture`

Expected: the old completion is accepted or the pending revision is lost.

- [ ] **Step 7: Add exact identity validation and lossless requeue**

For each occupied slot require live current status, matching key/block generation/light revision, and `Arc::ptr_eq` for both light and direct-sky mask. On any stale condition call a helper that re-inserts only the completion revision when it remains the current dirty revision and no newer pending entry exists.

- [ ] **Step 8: Run GREEN and bounded mesh regressions**

Run: `cargo test -p bedrock-client stale_light_halo_mesh_completion --locked`

Run: `cargo test -p bedrock-client mesh_dispatch_never_exceeds_the_bounded_worker_window --locked`

Expected: rejection/no-loss tests pass and the 128-job mesh window remains binding.

### Task 4: Full 26-neighbour light lifecycle invalidation

**Files:**
- Modify: `app/src/world_stream.rs`
- Modify: `crates/world/src/chunk.rs` only if a checked helper is required beyond existing `mesh_neighbourhood_dependents()`.

**Interfaces:**
- Consumes: `SubChunkKey::mesh_neighbourhood_dependents` and existing revision coalescing.
- Produces: one `mark_light_mesh_dependents(key, since)` helper used for current light commits, load transitions, removals, and evictions.

- [ ] **Step 1: Write failing commit/load/eviction tests**

For a coordinate-safe source, assert the pending mesh key set equals the 27 checked dependants after:

```rust
let expected = source.mesh_neighbourhood_dependents().collect::<BTreeSet<_>>();
assert_eq!(pending_keys(&stream), expected);
```

Exercise first current light commit, changed replacement, known-air/resident load, and column eviction. Repeat the same event twice and assert the `HashMap` remains 27 entries with current revisions rather than growing duplicates.

- [ ] **Step 2: Run RED**

Run: `cargo test -p bedrock-client light_lifecycle_invalidates_full_mesh_halo --locked -- --nocapture`

Expected: current code dirties only the center or face-limited dependants.

- [ ] **Step 3: Implement lifecycle invalidation**

Dirty every checked dependent that can own a mesh, using existing revision tracking and earliest `since` semantics. Call the helper exactly when light becomes current/changes and before removal or eviction discards the old identity. Do not widen block-data face culling rules or liquid dependency masks.

- [ ] **Step 4: Run GREEN and coalescing/bounds tests**

Run: `cargo test -p bedrock-client light_lifecycle_invalidates_full_mesh_halo --locked`

Expected: all lifecycle cases produce the exact bounded 27-key set.

### Task 5: Honest plan evidence and complete verification

**Files:**
- Modify: `plan.md`

**Interfaces:**
- Consumes: measured test and benchmark output.
- Produces: an honest Phase 2.7 app-side mesh/light halo progress entry with remaining GPU/shader/live acceptance explicitly open.

- [ ] **Step 1: Run focused and full correctness gates**

Run:

```powershell
cargo test -p render -p bedrock-client -p world --locked
```

Expected: all render, app, world integration, and doc tests pass.

- [ ] **Step 2: Run the exact release throughput gate**

Run:

```powershell
cargo test --release -p bedrock-client --locked release_full_view_known_air_lighting_completes_within_two_seconds -- --ignored --nocapture
```

Expected: 26,136 known-air light completions finish within two seconds, all current, with zero stale results.

- [ ] **Step 3: Record only measured evidence in `plan.md`**

State the exact test counts and release elapsed time printed by Steps 1-2. Mark only the app-side fixed halo/sampler/identity subgate complete. Explicitly retain GPU arena/shader integration, mixed-block visual proof, and live full-view teleport/remesh acceptance as open.

- [ ] **Step 4: Run final formatting and lint gates**

Run:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

Expected: every command exits zero with no warnings or whitespace errors.

- [ ] **Step 5: Review scope and commit without pushing**

Run `git diff --stat 47ae126`, `git diff --name-only 47ae126`, and inspect the full diff. Commit only render sampler, app halo/scheduler tests, any necessary world checked helper, design/plan docs, and `plan.md`. Do not push.
