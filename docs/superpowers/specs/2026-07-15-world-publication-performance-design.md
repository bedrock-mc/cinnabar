# World Publication Performance Design

## Goal

Make initial radius-16 publication and teleport recovery settle within two seconds without remeshing unchanged light results, while retaining fail-closed generation checks and exact lighting.

## Root-cause evidence

The release-only light scheduler benchmark solves all 26,136 known-air subchunks in about one second, so the solver alone is within budget. Affected live runs publish only a fraction of the view after minutes and spend roughly 40–48 ms per frame despite 5–10 ms GPU passes. In the current completion path, the worker already computes `light_levels_changed`, but the app discards it and unconditionally calls `mark_light_mesh_dependents`, dirtying the center and all 26 halo consumers for every accepted completion. An unchanged solve therefore creates revision churn, stale mesh completions, queue work, and uploads that cannot alter a vertex.

The observed slow settled path is also a debug-only Windows DX12 Direct-draw fallback; release MDI and presentation must be measured separately. This design does not treat that debug fallback as proof of a lighting defect, and it does not alter culling or presentation.

## Alternatives considered

Increasing worker counts or GPU-upload budgets would move the queues faster but would preserve redundant work and worsen frame spikes. Replacing the light engine or moving it to the GPU would abandon the reviewed sparse deterministic solver and is unsupported by evidence. Exact 27-region light-delta masks could reduce changed-light invalidation further, but they add a larger correctness surface. The first implementation therefore instruments the full path and removes only proven no-op publication; region-level invalidation is considered later only if release evidence still misses the gate.

## Design

`SolvedLightJob` will retain both value and direct-sky equality against its prior snapshot. A completion whose light nibbles and direct-sky mask are both unchanged clears the current dirty revision, preserves the existing `Arc<SubChunkLight>`, `Arc<DirectSkyMask>`, and light-value generation, updates ownership to the current block generation, wakes waiters, and performs zero mesh invalidations. It still records solve time and uniform-fast-path usage. Because dependency pointers remain stable, an unrelated in-flight mesh remains current.

When light values change, the existing generation-checked commit remains. A direct-sky-only change uses the already-proven single-in-flight-target invariant: after the same freshness checks, it preserves the old light `Arc` and value generation, replaces only the direct-sky mask tagged with that preserved value generation, advances ownership to the current block generation, and clears the current solver revision. Changed faces dirty affected light neighbours, which makes their already-running jobs stale before acceptance. It does not remesh because `MeshLightSampler` consumes block/sky nibbles and never consumes provenance. `MeshLightSlot` will stop retaining direct-sky pointer identity after the snapshot readiness gate, so provenance-only publication cannot reject a mesh whose sampled channels are unchanged. Any later nibble change still changes the light `Arc` and rejects/requeues stale work.

World-stream metrics gain cumulative accepted, no-op, value-changing, provenance-only, mesh-invalidated, and stale completion counts plus queue-wait maxima for decode, light, and mesh stages. Render metrics already cover upload/submission; the acceptance log will join these with build profile, draw mode, present mode, backend, adapter, and driver from the visibility/presentation tranche. Counters remain bounded integers and logging remains periodic rather than per-subchunk.

## Correctness invariants

- A no-op completion may be accepted only after the existing target revision, block generation, residence, and previous light-generation checks pass.
- A no-op completion must leave stored light/direct-sky pointers and the sampled value generation unchanged.
- Ownership must use the current block generation so an equal-output block replacement becomes current without another solve.
- Waiters and changed-face neighbour scheduling retain their current ordering and bounds.
- Direct-sky-only changes must be visible to subsequent light jobs, retain the old nibble generation, dirty every neighbour named by the changed-face summary, and produce no mesh revision or upload.
- Any actual nibble change must invalidate every currently required mesh halo consumer; this tranche does not narrow that set.
- Eviction, dimension replacement, stale completion, fatal solver failure, and retry behavior remain unchanged.

## Verification

TDD tests cover unchanged uniform and packed results, equal output after block-generation replacement, direct-sky-only changes, actual value changes, concurrent in-flight meshes, waiters, stale/fatal paths, and counter accounting. A release benchmark exercises radius 16 through light completion, mesh readiness, mesh completion, render queue extraction, and upload acknowledgement rather than stopping at the solver. The live BDS gate records stage latency and requires full publication after join/teleport within two seconds, no visible stalls, combined RSS at most 650 MB, and settled CPU at most 15% on the reference class. FIFO and Immediate runs remain separately labelled.
