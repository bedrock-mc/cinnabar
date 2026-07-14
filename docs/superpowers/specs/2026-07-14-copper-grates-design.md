# Exact copper-grate design

## Goal

Render the eight stateless protocol-1001 copper-grate records as vanilla
alpha-cutout full-cube models while preserving holes, open cave connectivity,
exact-state internal-face culling, wax/oxidation boundaries, and full collision.

## Selected architecture

Reuse the checked six-quad transparent-cube template semantic introduced for
ordinary stained glass. Generalize its trust boundary from all-alpha-blend to a
homogeneous alpha class: every referenced material must be non-diagnostic and
all six must use exactly one of alpha blend or alpha cutout. Mixed classes,
both alpha bits, and opaque materials remain invalid.

The alternative opaque greedy route would fill the grate holes and close cave
connectivity. A dedicated grate pipeline would duplicate the existing
depth-writing cutout model path. Neither is acceptable.

## Exact admission

Admit only canonical state `{}`, `ModelFamily::Cube`,
`ContributorRole::Primary` records named:

- `minecraft:copper_grate`
- `minecraft:exposed_copper_grate`
- `minecraft:weathered_copper_grate`
- `minecraft:oxidized_copper_grate`
- `minecraft:waxed_copper_grate`
- `minecraft:waxed_exposed_copper_grate`
- `minecraft:waxed_weathered_copper_grate`
- `minecraft:waxed_oxidized_copper_grate`

Require the checked production cube facts. Resolve the pinned Mojang texture
alias and set alpha cutout only; none is animated. Waxed variants intentionally
share the corresponding unwaxed texture. Compile one exact six-quad unit cube
and clear runtime `AIR | CUBE_GEOMETRY | OCCLUDES_FULL_FACE | LEAF_MODEL`.
Registry collision facts remain untouched.

Slime, stained/hard glass, panes, copper bars/bulbs/doors/trapdoors, unrelated
`*grate*` names, legacy flags-zero records, and invisible bedrock remain
excluded.

## Meshing

Internal-face suppression for the checked transparent-cube semantic must use
exact `ResolvedPaletteEntry.network_value` equality, not material equality.
That is stable in sequential and hashed network-ID modes and is equivalent for
ordinary stained-glass colours. It deliberately retains boundaries between
waxed and unwaxed grates even where all six materials alias the same texture.

Identical grate states emit 10 opaque-model cutout draw refs for two adjacent
blocks; different oxidation or wax states emit 12. An opaque neighbour hides
the grate face behind it while retaining its own face. Grates remain cave-open
and the rule works across all six subchunk boundaries. No render pipeline,
bind-group, allocation, upload, or sorting change is required.

## Verification

- Test-first exact eight-record registry and pinned-pack compiler coverage.
- Exact unit-cube geometry/UV/cutout flags, four wax alias pairs, deterministic
  record reordering, and strict exclusions.
- Checked blob/runtime rejection for mixed blend/cutout, both alpha bits,
  opaque/diagnostic materials, incompatible flags, wrong topology/count.
- CPU mesh tests for identical/different oxidation, waxed-unwaxed equal-texture
  boundaries, opaque asymmetry, cave connectivity, cutout-only ordinary model
  routing, both network-ID modes, and all six subchunk boundaries.
- Production ratchet removes exactly eight intended IDs with zero additions,
  reducing 7,706 diagnostics to 7,698; gallery inventory uses the same count.
- Full assets/render/visualcoverage suites, pinned real-pack tests, explicit
  ratchet/gallery tests, strict Clippy, formatting, diff, review, and push.

Native screenshots remain in the shared Phase 2.6 gallery/live gate. No Mojang
asset or compiled blob is tracked.
