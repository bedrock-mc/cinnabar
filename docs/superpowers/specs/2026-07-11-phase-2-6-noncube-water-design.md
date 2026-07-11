# Phase 2.6 Non-Cube Models, Water, and Flipbooks Design

Date: 2026-07-11

Status: approved by the standing instruction to continue toward visible vanilla
parity, with natural-world impact prioritized before low-frequency decorative
models.

## Outcome

Phase 2.6 removes the diagnostic-magenta fallback from every non-air canonical
Bedrock state by adding the missing visual classes without abandoning the
rendering performance playbook. The order below prioritizes live visual impact,
but is not a residual allowlist:

1. animated, biome-tinted water;
2. generic flipbook textures;
3. crossed cutout plants and crops;
4. compact static model templates, beginning with slabs and stairs;
5. doors, trapdoors, panes, and fences;
6. static chest and sign models;
7. every remaining model family found by the exhaustive 16,913-state coverage
   report, including walls, beds, carpets, ladders, rails, torches, lanterns,
   cakes, hoppers, redstone-like overlays, and low-frequency decorative blocks.

The phase preserves palette-native chunk storage, the existing eight-byte greedy
cube record, shared texture-array resources, bounded worker/upload queues, direct
and multi-draw-indirect parity, and deterministic fail-closed diagnostics.

## Evidence and source boundaries

The design is based on current pinned inputs rather than an assumed Bedrock model
format:

- The pinned Mojang resource pack has 1,231 `blocks.json` entries, 1,300 terrain
  texture keys, and 83 flipbooks. It has no block-render model JSON. Its `models`
  directory contains entity geometry only.
- The behavior pack has only 57 `minecraft:voxel_shape` files. These are partial
  collision/occlusion shapes, not a complete state-to-render-model catalog.
- Axolotl Stack commit `6f6806e821a579c183c44d786f76d9b358a2b825`
  has no renderer, texture-pack reader, water pipeline, or block meshes. It does
  provide a useful versioned, typed state-selector pattern in Valentine. Its
  generated `v1_26_30` data is not a complete protocol-1001 palette authority:
  it contains 15,845 palette entries and 1,321 block definitions, versus the
  canonical 16,913 states and 1,356 names below. This is the current upstream
  `main` commit (merged PR #3), not a stale local checkout.
- PMMP BedrockData commit `bdb44a48fb6beffb6e9f6864f06d2232eb62b6a3`
  targets Bedrock 1.26.30 / protocol 1001 exactly. Its 1,356 block-property
  records, 16,913 canonical state metadata entries, and 88 biome definitions
  match the current generated registry/blob cardinalities. It is a strong CC0
  palette, property, and biome authority, but contains no textures or render
  geometry.
- PrismarineJS minecraft-data commit
  `6ec59288287e4045331eaa47ee8fb104278f6b98`, which is the exact submodule pin
  used by Axolotl, contains a Bedrock 1.26.30 `blockCollisionShapes.json`. It maps
  all 1,356 block names and 16,913 states to 342 reusable AABB collision shapes
  (12,513 non-empty states, 4,400 empty states, 23 multi-box shapes, at most
  seven boxes). Axolotl's generator does not ingest this file. Collision shapes
  are valuable template bounds and occlusion evidence, but are not render
  geometry or UV layouts.
- Dragonfly supplies open-source block-family behavior and bounding-box/model
  semantics for many stateful families. Like Prismarine collision shapes, this
  is an input to a reviewed exporter, not an assertion of exact visible geometry
  and not a runtime dependency.
- Zuri is not a design input. Its incomplete per-chunk renderer does not satisfy
  the project's memory or rendering architecture.

Source responsibilities are therefore explicit:

| Concern | Authority |
|---|---|
| Runtime palette and canonical state ordering | pinned PMMP `canonical_block_states.nbt`/`block_state_meta_map.json`, joined state-for-state to the current Dragonfly export and pinned Prismarine state catalog; every joinable Axolotl Valentine typed range is cross-checked as an explicitly incomplete overlap catalog |
| State selectors and family classification | generated, versioned registry metadata following Axolotl's typed mixed-radix approach and checked against PMMP's exact canonical span |
| Collision/selection bounds and cuboid-template seeds | pinned PrismarineJS Bedrock collision shapes plus Dragonfly `BBox` results, retaining source and confidence per family |
| Visible model geometry, neighbor rules, and UVs | deterministic local family generators, Mojang texture mappings, Dragonfly behavior rules, and per-family vanilla-reference review; collision boxes alone are never labeled render-authoritative |
| Textures, face aliases, variants, flipbook frames, and biome colors | pinned Mojang resource pack |
| Block opacity/brightness and biome-definition cross-checks | pinned PMMP `block_properties_table.json` and `biome_definitions.json` |
| Opaque/cutout/blend classification | explicit reviewed family rules plus decoded alpha evidence; neither PMMP opacity nor Axolotl's coarse `IS_TRANSPARENT` field is sufficient alone |

No Mojang image or pack payload is committed. Compiled runtime assets remain
local ignored outputs. Generated metadata checked into the repository contains
only identifiers, state selectors, geometry/UV descriptors, hashes, and
provenance allowed by the respective source licenses. PMMP BedrockData is CC0;
PrismarineJS minecraft-data and the applicable Axolotl/Dragonfly sources are MIT
and retain their notices; separately licensed subtrees and upstream datasets
retain their own terms.

## Why the current screen is pink

The current `MCBEAS03` blob contains 16,913 visuals, but only 661 (3.91%) have
real materials. The other 16,252 are wholly diagnostic. Of those, 16,199 have
`BlockFlags::NONE`: the current mesher treats each non-air non-cube state as six
unculled diagnostic cube faces. The reproducible inputs are registry SHA-256
`8a27e1389f5ffa2e2ab032563a45660dc31f5d708fdacb2225b344a49aa15bfc`, blob
SHA-256 `1fbd361c489d3cf90edb49c0056b83ffd9a2a114a36ac1eaf28cfd1103ecf508`,
HEAD `00b7a32ea948f55235f68b13505831d3ec611135`, and acceptance artifact
`.local/acceptance/20260711T192110Z-16912/app.stdout.log`. That run recorded
849,117 diagnostic quads across 9,040 resident and 7,093 visible subchunks.

The runtime's `missing_mapping_count=0` means state IDs resolved to visual
records; it does not mean those records have a real model or material.

## Selected architecture

Use hybrid template-reference streams. Keep the existing `PackedQuad` exactly
eight bytes for binary-greedy cube faces. Add compact stream-specific records
rather than expanding reusable model geometry into every chunk.

### Asset schema: `MCBEAS04`

Version the runtime blob rather than repurposing reserved `MCBEAS03` fields.
All sections use checked offsets, counts, byte sizes, canonical ordering, and
explicit upper bounds.

New or expanded sections are:

- visual records that select cube, cross, model-template, or liquid behavior;
- model-template descriptors and immutable template quads;
- animation descriptors and frame-layer indices;
- expanded material records carrying render class, tint class, and optional
  animation ID;
- texture-page descriptors and one or two bounded texture-array mip payloads,
  containing every physical animation frame as an ordinary array layer.

Before the texture representation is frozen, the real compiler emits an exact
inventory of referenced static layers, physical animation frames, deduplicated
layers, and adapter limits. The pinned source has 1,281 unique terrain paths and
roughly 1,209 physical flipbook frames before reachability and byte-identity
deduplication, so fitting the current 2,048-layer ceiling is not assumed. If the
deduplicated reachable set exceeds the minimum target adapter's per-array limit,
the bounded fallback is two identically configured texture-array pages in the
same bind group, selected by a material page bit. Stitched atlases remain
forbidden. More than two pages, frame dropping, or silent animation degradation
is a compile error.

`MCBEAS04` defines a canonical `TextureRef(u32)`: bit 31 is the page (0 or 1),
bits 0–10 are the layer (0–2047), and bits 11–30 must be zero. Materials and
animation frame tables both store `TextureRef`, not bare layers. Each page
descriptor carries dimensions, layer/mip counts, checked payload offset/length,
and payload hash. A one-page asset binds a one-layer diagnostic second page so
the GPU layout is identical. Blended animations may cross pages; the shader
selects each referenced page independently before interpolation, and codec/GPU
tests cover cross-page current/next pairs.

The decoder rejects overlapping sections, unknown flags, invalid indices,
non-canonical ordering, arithmetic overflow, excessive counts, and payload/hash
mismatches. The current 16 MiB startup limit is replaced only after measuring a
real compiled asset and selecting a documented bounded ceiling.

### Generated registry metadata

`tools/registrygen` emits versioned, reviewable metadata for:

- model family;
- typed state parameters (orientation, half, open, hinge, connection mask,
  growth stage, and liquid depth as applicable);
- primary occlusion contributor;
- optional liquid/additional-layer contributor;
- conservative full-face coverage flags.

The generator follows Axolotl Valentine's state-range and mixed-radix validation
concepts, but the project owns its generated schema. PMMP, Dragonfly, and
Prismarine records join by canonical namespace-qualified name plus a canonical
typed state compound whose keys are sorted and whose scalar types and values are
preserved. Generation requires a complete 16,913-state bijection across those
three sources: duplicate keys, unmatched records, hash collisions, order-only
matches, or unequal state values fail with attributable diagnostics. Cardinality
equality alone is never accepted as identity. The pinned Valentine catalog is a
separately reported overlap audit: every joinable Valentine record must match the
same canonical typed state exactly, but its evidenced 15,845-entry/1,321-block
scope is not misrepresented as complete and its missing canonical states are not
synthesized. Generation also fails if any selector cardinality disagrees with
the canonical state span it claims to cover.

### Palette-native contributor resolution

Replace “first non-air storage layer wins” with a per-coordinate resolver that
reads packed palette indices directly and returns:

- at most one primary solid/model contributor; and
- zero or more explicitly supported additional contributors, initially liquid.

This supports waterlogged blocks without expanding a subchunk into a flat 4,096
block array. All resolution happens on Rayon workers from immutable bounded
snapshots.

The resolver examines all accepted storage layers in protocol order (currently
bounded to 16). Air is ignored. The lowest-ordinal primary candidate is selected
only if it is the sole primary candidate; two solid/model candidates are a
conflict even when equal. One liquid contributor may appear before or after the
primary. Exact duplicate liquid states collapse deterministically, while distinct
liquids conflict. An unclassified non-air additional contributor, more than one
primary, or incompatible liquid combination emits one attributable diagnostic
contributor for that coordinate and no potentially incorrect real geometry. A
liquid without a primary is valid. Tests cover every conflict and ordering class.

### Geometry streams

`ChunkMesh` contains independent streams with atomic generation identity:

- greedy cube quads: existing eight-byte `PackedQuad`;
- model references: compact per-block transform plus global template reference;
- liquid quads: compact fixed-point surface/side geometry including corner
  heights and flow direction;
- transparent draw references: per-view back-to-front indirection into model or
  liquid records.

Template geometry is uploaded once globally. Subchunks upload references, not
duplicated vertices. `PackedModelRef` is 16 bytes: packed local position/transform,
template ID, lighting-base index, and a 32-bit visible-quad/variant mask. A
template is bounded to 32 quads; more complex models split deterministically into
multiple references.

Lighting is face- and template-vertex-specific, not shared only by spatial block
corners. Each template quad has one eight-byte `PackedQuadLighting` sidecar in
template-quad order, even when its visibility bit is clear. It contains four
`u16` vertex samples; each sample stores block light in bits 0–3, sky light in
bits 4–7, AO in bits 8–9, and requires bits 10–15 to be zero. Rayon workers bake
these values from the face-specific neighbor set at mesh time. Each liquid quad
indexes its own `PackedQuadLighting`, covering the actual four vertices of top,
bottom, or side geometry. This GPU addressing is established now so Phase 2.7
does not widen or replace the model streams. Direct and MDI use identical
reference/quad-light indexing. Every stream and sidecar has explicit byte
accounting in the pending queue and GPU arenas.

Before Phase 2.7 installs block/sky flood-fill values, Phase 2.6 uses the explicit
temporary input block light 0 and sky light 15 while still baking face-specific
AO. Phase 2.7 changes only those inputs and invalidates/remeshes affected chunks;
the sidecar format, ordering, and GPU addressing remain unchanged.

The presented-frame contract changes from one draw bit to an expected/drawn
stream mask. A subchunk generation is acknowledged only after every non-empty
stream for that generation has been emitted. Direct and MDI paths consume the
same allocation descriptors and produce equivalent record addressing.

### Pipelines and texture resources

Retain one logical texture-array resource (one physical page when measured limits
permit, otherwise the bounded two-page fallback above), one sampler,
material/animation buffers, and one shared bind group.

- Opaque/cutout uses the current reverse-Z opaque phase, depth test/write, and
  cutout discard.
- Blend uses Bevy's transparent phase, straight-alpha blending, reverse-Z depth
  test, and no depth writes.

These are two immutable state variants of one chunk pipeline family; the binding
playbook's single-pipeline rule is narrowed accordingly because blend and depth
writes cannot be dynamic state. Both variants share the bind group, shaders'
resource architecture, arenas, visibility system, and direct/MDI manifests.

Transparent work is bounded to visible transparent records. Rayon workers sort
subchunks back-to-front and build per-subchunk face order using world-space
centroids into a double-buffered compact indirection stream. The hard ceiling is
2,097,152 references (16 MiB at eight bytes each); excess is an attributable
acceptance failure, not silently dropped geometry. Sort output uses the same
per-frame upload cap as meshes, the previous valid order remains active until a
complete replacement is uploaded, and camera-motion acceptance measures the
sort CPU, bytes, latency, and frame p99 against the 60 fps / 15% steady-CPU gate.
This is vanilla-compatible depth ordering, not a claim of mathematically exact
ordering for arbitrary intersecting translucent polygons. Equal compatible blend
groups suppress internal shared faces; blend geometry never hides opaque
geometry.

Every sort request carries a monotonically increasing `ViewSortGeneration`
covering the quantized camera pose, visible allocation set, asset revision, and
mesh generations. A worker result commits only when its full key equals the
current requested key; late results are discarded before upload. Direct and MDI
consume the same committed ordered snapshot, and transparent presented-frame
accounting records the committed view-sort generation. Tests force out-of-order
worker completion and visibility/camera changes.

### Flipbooks

The pack reader preserves and validates all 83 flipbook fields that affect
rendering: frame order, ticks per frame, atlas index/tile variant, replication,
and `blend_frames`.

The compiler slices each physical frame into its own texture-array layer and
emits immutable animation descriptors. The shader selects frames from a small
global animation clock; blended animations sample adjacent layers. Animation
never causes per-frame texture uploads.

### Water

Water is an engine-owned liquid model, not a cube or static template.

The liquid mesher performs:

- same-liquid internal-face culling;
- top suppression when compatible liquid exists above;
- state-derived surface levels;
- vanilla-like four-corner height calculation;
- clipped side faces;
- still-versus-flow animation selection and flow direction;
- biome water tint and pack-derived surface alpha.

Corner heights require diagonal horizontal samples. Mesh snapshots and mutation
invalidation therefore cover a bounded horizontal 3x3 neighborhood plus the
vertical samples used by the algorithm. A diagonal liquid change must invalidate
every mesh whose corner heights depend on it.

### Cross plants and crops

Plants use a reusable crossed-quad template with cutout materials, two-sided
visibility, biome tint where appropriate, and deterministic state-to-texture
variant selection. The compiler handles legacy aliases and growth-stage arrays
explicitly. This slice covers common grass, flowers, saplings, ferns, crops,
seagrass, and kelp before rarer decorative models.

### Static model families

Static models use immutable fixed-point template quads and compact instance
references. Initial culling is conservative: only a proven opaque full-face
neighbor hides a template face, and hidden faces within a template are removed
at compile time. Partial models remain connectivity-open until a separately
verified face-coverage optimization exists.

Implementation order is:

1. cuboid template IR, slabs, and stairs;
2. doors and trapdoors;
3. connection-aware panes, fences, and gates;
4. static chest and sign models.

The first four groups are followed by the exhaustive residual-family report
until every canonical non-air state has a non-diagnostic visual. Cuboid templates
may be seeded from Prismarine/Dragonfly collision boxes only when a family review
shows the visible bounds match; every family records the geometry source,
procedural rules, UV rule, source license, and vanilla-reference gallery. Collision
boxes that simplify visible geometry (for example chests) are not promoted as
render models.

Neighbor-dependent families receive bounded neighbor masks in their model key;
they do not query live world state from the render thread.

## Work and upload budgeting

Decode, contributor resolution, liquid calculation, and meshing run on Rayon
workers. Results are nearest-first and generation-gated. GPU uploads retain the
per-frame cap, now summed across all streams. Allocation of every required range
for one subchunk is atomic: failure queues the whole generation for retry rather
than exposing partial geometry.

Texture/model tables are immutable for an asset revision. A hot-swap installs a
new complete revision only after validation, then invalidates dependent meshes.
Stale worker completions and stale prepared GPU records are rejected by the
existing stream/revision/generation identity gates.

## Failure behavior

Unsupported or malformed visual records remain conspicuously diagnostic during
development, but a single bad state cannot corrupt adjacent records or alias a
valid material. Diagnostics include per-family and per-material live
quad/instance counters so the next parity gap is attributable by canonical block
name/model family rather than inferred from screenshots. Phase 2.6 completion
requires zero diagnostic visuals for all non-air canonical states and zero live
diagnostic geometry; any explicit residual allowlist must be assigned to and
implemented by a named Phase 2 task before the phase can close.

Missing local Mojang assets remains a clean startup error with acquisition
instructions. The codebase and tests continue to build without Mojang files.

## Verification gates

Each implementation task follows red-green-refactor and adds deterministic
fixtures before production code.

Required automated evidence includes:

- exact `MCBEAS04` round trips plus malformed/overflow/limit fixtures;
- state-selector cardinality and known state-family fixtures;
- flipbook frame ordering, timing, replication, interpolation, and adapter-limit
  fixtures;
- cross-plant variant and tint fixtures;
- palette-native multi-layer contributor tests;
- water corner-height, same-liquid culling, diagonal invalidation, tint, and
  transparent ordering tests;
- model transform/UV/culling fixtures for every added family;
- exact PMMP/Dragonfly/Prismarine state-by-state bijection fixtures plus an
  attributable Valentine overlap/cardinality-deficit audit;
- exhaustive 16,913-state visual coverage with every non-air state assigned a
  non-diagnostic visual kind; source-backed vanilla-invisible/engine-only kinds
  may intentionally emit no geometry but cannot use the diagnostic fallback;
- unchanged eight-byte `PackedQuad` assertion and bounded record-size assertions;
- model/light-sidecar and liquid-light addressing parity in direct and MDI paths;
- texture-page codec validation and same-page/cross-page animation sampling;
- multi-layer primary/liquid ordering, duplicate, and conflict fixtures;
- stale `ViewSortGeneration` rejection and direct/MDI ordered-snapshot parity;
- direct/MDI addressing parity and multi-stream presented-frame tests;
- queue byte-bound, atomic allocation, stale-revision, and retry tests;
- clean-no-assets workspace tests, formatting, strict Clippy, and all-target
  workspace checks.

Live acceptance uses the current BDS scene plus deterministic galleries. Native
Windows screenshots are used while the Computer Use capture backend is broken.
Evidence must show:

- zero diagnostic visuals across the exhaustive canonical-state report and no
  diagnostic magenta/geometry in representative terrain;
- animated and biome-tinted water from multiple angles;
- correct plant/crop textures and cutout edges;
- slab/stair inner/outer corners, doors, trapdoors, panes, fences, gates,
  waterlogging, liquid flow direction, blended flipbooks, chest/sign static
  geometry, and every state/neighbor variant in the deterministic galleries;
- movement, mouse look, focus, and multiple viewing distances;
- title-bar FPS plus fresh RSS, steady CPU, upload-budget, and teleport-remesh
  measurements.

Double-chest joining and rendered sign text are block-entity custom behavior and
remain deferred; single/static chest geometry and blank sign boards are required
here. Phase 2.6 is not complete until all non-air diagnostic counters are zero in
the exhaustive report and deterministic galleries, and the live screenshots
agree with the pinned vanilla references. Broader Phase 2 completion still
requires Phase 2.7 lighting/atmosphere and the final acceptance gates.
