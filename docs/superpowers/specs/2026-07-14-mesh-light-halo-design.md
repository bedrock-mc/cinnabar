# Mesh Light Halo Design

## Goal

Replace the Phase 2.6 constant mesh-light inputs with current palette-native
client light snapshots while preserving bounded worker scheduling and avoiding
flat 4,096-sample staging arrays.

This slice starts from exact commit `47ae126085b5ec4e6c54683fc9f130d2af2f45ea`.
It does not change GPU arenas, shaders, draw formats, assets, or block-family
rendering.

## Selected architecture

The render crate owns a small read-only sampler contract. A light sample
contains only the block and sky nibbles consumed by `PackedQuadLighting`.
Direct-sky provenance is not a render channel and never enters packed geometry.

The app owns a fixed 27-slot `MeshLightHalo`, indexed by the existing canonical
`[-1, 1]^3` mesh-neighbourhood mapping. Each occupied slot captures:

- the exact `SubChunkKey`;
- an `Arc<SubChunkLight>`;
- the owning block generation and light revision;
- an `Arc<DirectSkyMask>` and its matching light revision; and
- whether the scheduler considered the slot current and trusted at capture.

`MeshSnapshot` owns this halo beside the palette-native block neighbourhood.
The halo routes signed center-relative coordinates with Euclidean division and
remainder directly into one slot and one nibble read. It creates no per-voxel
array or map. Coordinates outside the 27-slot cube, absent slots, and unknown
boundaries return block zero and sky zero.

## Render interface

`render::LightSampler` exposes one allocation-free method:

```rust
fn sample_light(&self, coordinate: [i32; 3]) -> LightSample;
```

`LightSample` validates or masks its two four-bit channels and provides a dark
fallback. Existing AO block-occlusion sampling remains palette-native through
`MeshNeighbourhood`. Model and liquid lighting helpers accept a sampler and
replace only the Phase 2.6 block/sky constants. The packed sidecar format is
unchanged.

## Dispatch and identity rules

Before dispatching a resident center mesh, the app determines every light slot
that is known/resident inside the fixed halo. Every such slot must pass the
existing `light_is_current` ownership, kind, generation, and direct-sky checks.
If any consumed known slot is not current, mesh dispatch leaves the current
pending entry intact. Unknown/absent slots are allowed and sample dark.

The worker completion returns the captured halo identities with the mesh. On
acceptance, every occupied identity is compared with live state by exact key,
block generation, light revision, `Arc::ptr_eq` for both the light volume and
direct-sky mask, and current/trusted status. Any mismatch rejects the mesh.

A rejected completion removes only its matching in-flight marker, increments
the stale-mesh counter, and ensures that the still-current mesh revision remains
pending. It never loses work merely because the original pending entry was
removed at dispatch. A newer block revision remains authoritative and is not
overwritten by the rejected completion.

## Invalidation and ordering

Whenever a light volume changes, first becomes current, is removed, or is
evicted, the app dirties every checked `mesh_neighbourhood_dependents()` target:
the source plus all 26 face, edge, and corner dependants. The existing
`HashMap<SubChunkKey, PendingMesh>` coalesces duplicates and the existing worker
and result capacities remain unchanged.

Both completion orders converge:

- If light completes before mesh dispatch, the mesh captures current identities.
- If a mesh is pending first, dispatch waits until all known halo light is current.
- If any halo light is replaced while a mesh worker runs, acceptance rejects and
  requeues the mesh revision against the new halo.
- Load and eviction invalidate the full dependent halo so an absent fallback can
  neither hide newly known light nor preserve removed light.

## Tests and acceptance

TDD coverage will prove:

- center, face, edge, and corner coordinate routing;
- dark fallback for absent and out-of-halo samples;
- block and sky nibble propagation without exposing direct-sky to render;
- light-first and mesh-first completion ordering;
- dispatch gating on every known/resident halo slot;
- mid-flight light replacement rejection and exact requeue;
- load and eviction invalidation of all 26 dependants;
- no lost rejected mesh revisions;
- pending-map coalescing and existing worker/result bounds; and
- unchanged release light-scheduler throughput gate.

Final verification includes focused render/app/world tests, full app/world tests,
the exact ignored release workload test, `cargo fmt --all -- --check`, and strict
workspace Clippy with `-D warnings`. `plan.md` will record only evidence actually
measured by those commands and will keep GPU/shader integration and live full-view
remesh acceptance open.
