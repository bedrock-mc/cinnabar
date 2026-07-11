# Cutout Cube Leaves Design

**Date:** 2026-07-10

**Branch:** `phase2-leaf-cutout`

**Status:** Approved

## Outcome

Render Dragonfly `model.Leaves` blocks as alpha-tested, depth-writing cubes in the existing
packed chunk pipeline. Leaves gain real texture holes, correct asymmetric face culling, and
open cave-connectivity behavior without widening the material or quad records, adding a render
phase, or expanding paletted subchunks into flat block arrays.

This slice validates self-colored `minecraft:cherry_leaves`, `minecraft:azalea_leaves`, and
`minecraft:azalea_leaves_flowered` first. Other leaf textures may remain grayscale until the
separate biome-data and foliage-tint slice. A grayscale common leaf with correct cutout geometry
is acceptable here; a claim of common-leaf color parity is not.

## Scope

The scope is cutout, axis-aligned cube leaves only. It does not include cross plants, general
block models, water or other blended materials, biome data or tint lookup, block or sky
lighting, animation, sky, fog, or clouds. Those features retain their existing Phase 2
assignments.

Unsupported non-air models continue to use the conspicuous diagnostic cube fallback so
coverage gaps remain measurable. That fallback is not allowed to become a full-face
occluder or a cave-connectivity blocker. Missing runtime mappings remain bounded, counted, and
diagnostic rather than indexing asset or GPU tables unchecked.

## Versioned block semantics

The current `FULL_CUBE` bit conflates texture eligibility, geometry, face occlusion, and cave
closure. Replace it with four independent `BlockFlags: u8` facts:

```rust
bitflags! {
    pub struct BlockFlags: u8 {
        const AIR = 1 << 0;
        const CUBE_GEOMETRY = 1 << 1;
        const OCCLUDES_FULL_FACE = 1 << 2;
        const LEAF_MODEL = 1 << 3;
    }
}
```

The pinned Dragonfly exporter assigns them as follows:

- `minecraft:air`: `AIR` only.
- `model.Solid` and the existing explicitly vetted unknown mycelium/huge-mushroom states:
  `CUBE_GEOMETRY | OCCLUDES_FULL_FACE`.
- `model.Leaves`: `CUBE_GEOMETRY | LEAF_MODEL`, never `OCCLUDES_FULL_FACE`.
- Other known models: no cube/occlusion/leaf flag.

`AIR` may not be combined with another flag. `OCCLUDES_FULL_FACE` and `LEAF_MODEL` each imply
`CUBE_GEOMETRY`, and `LEAF_MODEL | OCCLUDES_FULL_FACE` is invalid. The Go exporter, Rust
registry reader, compiler, blob encoder, and runtime loader enforce these combinations.

This changes meaning without changing protocol version, so schema identity must change. The
registry magic becomes `BREG1002`; the compiled asset magic becomes `MCBEAS02` and
`BLOB_VERSION` becomes `2`. Readers reject `BREG1001`, `MCBEAS01`, and version 1 rather than
guessing which flag meaning was intended. Filenames remain
`crates/assets/data/block-registry-v1001.bin`, `block-registry-v1001.sha256`, and
`vanilla-v1001.mcbea`: `v1001` identifies the pinned loopback protocol, not either binary schema.

## Asset compilation and cutout mips

`compile_pack` resolves real face textures for every `CUBE_GEOMETRY` record. For a
`LEAF_MODEL`, each resolved face material sets bit 8:

```rust
pub const MATERIAL_FLAG_UV_MASK: u32 = 0x0000_000f;
pub const MATERIAL_FLAG_ALPHA_CUTOUT: u32 = 1 << 8;
```

Bits 0 through 3 retain their current UV rotation/reflection meaning. No material bit in the
reserved gap 4 through 7 is assigned. `Material { layer: u32, flags: u32 }` remains exactly
eight bytes, and material deduplication includes the complete flags word so an alpha-tested
descriptor cannot alias an opaque descriptor accidentally.

The compiler continues to decode only referenced, bounded 16x16 PNG/TGA inputs and to build one
2D texture array. Mips remain isolated by array layer. For each layer used by an alpha-cutout
material, the base level's texel coverage at the shader cutoff (`alpha >= 128`) is the target for
every smaller mip. After the existing linear-light, premultiplied-alpha 2x2 downsample, a bounded
deterministic fixed-point search rescales only that mip's alpha channel. The target survivor
count is the base coverage rounded to the smaller mip's texel count; when equal-alpha ties make
that exact count impossible, the search chooses the nearest count and then the smaller scale.
The search uses a fixed iteration/range bound, performs no cross-layer reads, and leaves RGB and
opaque layers unchanged.

This coverage correction reduces distant leaf disappearance without turning the material into
blend. Tests use generated 16x16 patterns with different colors and coverage in adjacent layers,
assert each mip independently, and run the compiler repeatedly to prove byte determinism and
bounds.

## Palette-native meshing and visibility

The public mesher interface remains:

```rust
pub fn mesh_sub_chunk(
    classifier: &BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbours: &Neighbourhood<'_>,
    sub_chunk: &SubChunk,
) -> ChunkMesh;
```

It resolves each storage palette entry once into compact flags plus six material IDs, then reads
packed palette indices directly. It does not create `[u32; 4096]`, `[BlockFlags; 4096]`, or any
other flat per-block representation. Axis occupancy and culling stay as 16x16 arrays of `u64`
columns, and greedy merging still splits only when the visible face material differs.

Face culling is intentionally asymmetric. For source block `s` and adjacent block `n`, cull the
source face exactly when:

```rust
n.occludes_full_face() || (s.is_leaf() && n.is_leaf())
```

Therefore:

- an opaque full-face neighbor hides an opaque, leaf, or diagnostic source face;
- a leaf neighbor does not hide an opaque source face;
- two adjacent leaf cubes remove both shared faces, even when their materials differ;
- a leaf face touching an opaque neighbor is removed;
- a diagnostic/non-cube neighbor hides nothing.

The implementation forms geometry, occluder, and leaf `u64` masks and applies shifts and
AND-NOT operations per axis. Cross-subchunk boundary samples use the same predicate, so internal
and neighbor behavior cannot diverge. `PackedQuad { geometry: u32, material_id: u32 }` remains
exactly eight bytes, including for leaves.

Cave connectivity flood fill treats a voxel as open whenever it lacks
`OCCLUDES_FULL_FACE`. A uniform leaf subchunk is consequently all-connected even though it emits
outer leaf geometry; a uniform opaque subchunk remains closed. This is visibility connectivity,
not collision or lighting behavior.

## One opaque GPU phase

Cutout leaves remain in the shared depth-writing `Opaque3d` chunk pipeline, global quad/origin
arenas, one material buffer, one texture array, one sampler, one bind group, and existing
MDI/direct fallback. The vertex stage passes material flags flat to the fragment stage. The
fragment stage samples exactly once and applies:

```wgsl
let sampled = textureSample(block_textures, block_sampler, in.uv, i32(in.layer));
if ((in.material_flags & (1u << 8u)) != 0u && sampled.a < 0.5) {
    discard;
}
return vec4(sampled.rgb, 1.0);
```

There is no blending. Surviving leaf fragments are fully opaque and write the same depth as
ordinary chunk fragments, so cutout does not need transparent sorting or a second draw stream.
True blend remains deferred because partially transparent fragments require different ordering,
depth-write, and render-phase decisions. This slice must not introduce a per-subchunk Bevy
`Mesh`/`StandardMaterial`, a second texture array, a widened quad, or a second chunk pipeline or
bind group.

## Data flow

```text
pinned Dragonfly registry
  -> BREG1002 protocol-v1001 metadata with independent block flags
  -> local bedrock-samples compiler
  -> MCBEAS02 ignored blob with cutout material bit + coverage-preserving mips
  -> immutable RuntimeAssets
  -> palette-resolved u64 meshing and asymmetric culling/connectivity
  -> unchanged PackedQuad/global arenas/MDI
  -> one opaque shader sample, conditional discard, alpha-one output
```

## Local-only evidence gate

No Mojang archive, extracted file, PNG/TGA/JSON payload, derived texture pixels, or compiled
`.mcbea` blob is committed. Implementation regenerates and commits only the Dragonfly-derived
`block-registry-v1001.bin` and its SHA-256 metadata. The runtime blob is rebuilt under ignored
`.local/assets/compiled/vanilla-v1001.mcbea` from the already verified local source.

A deterministic BDS leaf gallery must include the three self-colored leaf names, opaque blocks
touching leaves, leaf-to-leaf adjacency, near/far cutout panels, and explicitly labeled common
leaves whose color parity is deferred. A deterministic forest fixture must exercise enough leaf
volume to measure the reduction in diagnostic quads against the pre-slice base. Both fixtures
publish exact commands, coordinates, camera pose, processing fence, and a hashed manifest before
screenshots are accepted.

The report records exact source/registry/blob/BDS hashes; registry records and flag counts;
visual/material/cutout-material/layer/mip byte counts; missing mappings; before/after diagnostic
quad counts and percentage reduction for the same forest fixture; frame/decode/mesh/remesh and
mutation latency; resident/visible counts; GPU upload bytes; combined client/core RSS; and steady
CPU. Common-leaf tint remains an explicit, named gap. Evidence is not accepted with untracked
asset payloads or without the final independent review.

## Computer Use limitation

The live pass attempts Computer Use first. When it succeeds, it repeats the active focus,
forward/back/strafe/vertical movement, mouse-look yaw/pitch, cursor-capture, Escape-release, and
no-stuck-input checklist. Task 8 previously activated the client window but its
required snapshot failed on Windows with
`SetIsBorderRequired failed: No such interface supported (0x80004002)`. If that exact limitation
recurs, no app input is sent after the observation failure. Passive GDI capture may record pixels
without changing app state, but it is labeled passive and cannot prove focus, keyboard/mouse
input, capture, or clean release. The deterministic server pose and prior Phase 0 interaction
evidence remain separate facts; neither is misreported as a fresh interactive cutout pass.

## Resource and verification gates

All existing bounded-work constants and nearest-first behavior remain in force, including
paletted world storage, `u64` binary greedy meshing, bounded worker result/request queues, the
64-job mesh dispatch budget, eight chunk GPU uploads per app frame, and the render queue's
256-item/64 MiB limits.

At radius 16 on the reference machine class:

- combined client + core RSS is at most 650 MB steady state;
- steady-state CPU is at most 15% total;
- join/teleport bursts settle within approximately two seconds;
- a full view-distance remesh after teleport is at most two seconds;
- modified-subchunk visibility latency remains at most 100 ms;
- the dev-MacBook authority retains the Phase 0 p99 frame-time gate of at most 8 ms;
- decode errors and missing mappings are zero or individually adjudicated and fixed before
  acceptance.

Unit and integration work follows RED -> GREEN -> REFACTOR. Schema tests reject old magic and
invalid flag combinations; compiler tests cover cutout assignment, deterministic output, layer
isolation, and alpha coverage; mesher tests cover every asymmetric pair in-subchunk and across
boundaries plus open leaf connectivity; shader tests prove one sample, bit-8 discard, alpha-one
output, no blend, depth writes, one pipeline/bind group, unchanged MDI selection, and exact record
sizes. Each task receives an independent review, and the complete branch receives a final
read-only review with all Critical and Important findings resolved.
