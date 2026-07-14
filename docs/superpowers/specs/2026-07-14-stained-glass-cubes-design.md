# Exact stained-glass cube design

## Goal

Remove the diagnostic visual from the 16 ordinary protocol-1001
`minecraft:<colour>_stained_glass` states while preserving vanilla
translucency, same-colour internal-face suppression, cross-colour boundaries,
open cave connectivity, and the existing bounded chunk-render architecture.

This tranche does not include Education `hard_*` glass, stained-glass panes,
copper grates, slime, or any legacy flags-zero record.

## Considered approaches

1. **Shared transparent model pipeline (selected).** Compile each exact stateless
   stained-glass record as one six-quad unit-cube model whose materials carry
   `MATERIAL_FLAG_ALPHA_BLEND`. Reuse the existing back-to-front transparent
   model phase and add one checked template semantic for same-material
   internal-face culling. This preserves the one-pipeline/one-bind-group design
   and keeps transparent cubes out of the opaque greedy stream.
2. **Opaque greedy cube.** This is smaller but wrong: it discards the texture
   alpha contract, closes cave connectivity, and cannot sort translucent faces.
3. **Dedicated glass pipeline.** This could specialize ordering, but duplicates
   the already-working transparent model path and adds pipeline/state complexity
   without a current requirement.

## Exact admission contract

- Admit only the 16 ordinary names:
  `black`, `blue`, `brown`, `cyan`, `gray`, `green`, `light_blue`,
  `light_gray`, `lime`, `magenta`, `orange`, `pink`, `purple`,
  `red`, `white`, and `yellow_stained_glass`, all in the `minecraft:`
  namespace.
- Require canonical state `{}`, `ModelFamily::Cube`, contributor role
  `Primary`, and the production registry's checked cube facts. Missing
  textures, extra state properties, wrong model family/role, or unrelated names
  remain diagnostic.
- Resolve the existing pinned Mojang texture alias per face and require an
  alpha-blended material descriptor. No Mojang payload is tracked.
- Compile a full `[0,0,0]..[256,256,256]` six-quad model. Clear
  `AIR`, `CUBE_GEOMETRY`, `OCCLUDES_FULL_FACE`, and `LEAF_MODEL` from
  the runtime visual so glass stays cave-open and never hides an adjacent opaque
  cube face.

## Meshing and rendering

Add one validated immutable model-template flag for transparent full cubes.
For each quad, retain ordinary culling against a neighbouring full opaque
occluder. Additionally suppress the shared face only when the neighbour carries
the same transparent-cube flag and the exact six-face material identity matches.
Different stained-glass colours retain both boundary faces. The rule applies
inside a subchunk and across all six subchunk boundaries.

All emitted stained-glass draw references enter the existing alpha-blended
transparent model list and its back-to-front, no-depth-write pipeline. No new
Bevy `Mesh`, material object, bind group, texture page, render phase, or
per-subchunk allocation is introduced.

## Trust boundaries

The new template flag is added to MCBEAS04 encoder/runtime validation. It is
valid only for one non-compound template with exactly six cuboid quads whose
materials are alpha blended. Malformed flags, wrong quad counts, diagnostic or
opaque materials, and incompatible combined flags fail decoding.

## Verification

- Test-first exact 16-name/state inventory and fail-closed exclusions.
- Exact six-face geometry, UV/material, alpha-blend, visual-flag, contributor,
  and deterministic-record-order tests.
- Blob/runtime malformed-template tests for the new semantic flag.
- CPU meshing tests for same-colour culling, different-colour retention,
  opaque/glass asymmetry, open connectivity, transparent-only draw references,
  and all six cross-subchunk boundaries.
- Pinned-pack compiler test and production ratchet: exactly 16 removals, zero
  additions, reducing the integrated 7,722 baseline to 7,706.
- Full assets/render/visualcoverage suites, explicit production ratchet, strict
  Clippy, formatting, and diff checks before review and push.

Native screenshots remain part of the shared Phase 2.6 gallery/live gate; this
tranche does not claim that external RX 570 presentation blocker is closed.
