# FlowerBed / Petals Vanilla-Parity Design

## Goal

Replace the incorrect full-height crossed-plant rendering of
`minecraft:wildflowers` and the diagnostic rendering of
`minecraft:pink_petals` with one compact, state-correct flowerbed family that
matches the pinned Bedrock client and preserves the Phase 2 packed-rendering
performance contract.

## Evidence boundary

- The pinned Bedrock registry proves 32 states per block: `growth=0..7` and
  four `minecraft:cardinal_direction` values.
- Geyser's state mapper proves normal gameplay maps flower amounts one through
  four to Bedrock growth zero through three; growth four through seven is
  command-only.
- Mojang's pinned Bedrock resource pack provides two texture layers per block:
  the four-quadrant flowerbed atlas and the tiny stem texture.
- Mojang's Java flowerbed models provide an exact additive geometry/UV baseline
  for normal amounts. They are a reference, not final proof of native Bedrock
  vertex identity.
- No pinned Axolotl, PMMP, Prismarine, Zuri, or bedrock-samples source publishes
  native Bedrock hard-coded flowerbed vertices.

## Architecture

Add a dedicated `FlowerBed` registry/model family for both names. It must not
reuse the generic full-height `Cross` family.

The asset compiler generates a small immutable template set from compact
quarter-patch primitives. A state selects an additive prefix of one through
four patches and applies cardinal rotation. Every patch uses a horizontal
flower plane plus its small stem planes, with the Bedrock flower and stem
texture-array layers kept distinct. Templates remain two-sided cutout geometry
in the existing packed model-ref path; no per-block Bevy mesh or material is
introduced.

Normal `growth=0..3` uses the exact Mojang flowerbed coordinates and UVs and
was checked against native Bedrock diagnostic-colour captures. The native
measurement gallery establishes the explicit layout mapping
`[0, 1, 2, 3, 3, 3, 3, 3]`: command-only `growth=4..7` each aliases the full
four-patch growth-3 layout for the same block and cardinal direction. The
compiler canonicalizes this measured layout before template lookup, so these
states add no duplicate templates. The measurement client is from the same
release line but is not the exact pinned preview build; exact pixel parity
remains gated on the matching client as recorded in the evidence document.

## Native Bedrock adjudication

Create a local-only diagnostic resource pack (never committed) that replaces
the flower atlas quadrants and stem pixels with opaque unique colours without
changing block definitions or geometry. A deterministic BDS gallery places all
64 states (two blocks, eight growth values, four directions). Capture top,
north, east, and oblique views in the pinned vanilla Bedrock client using fixed
camera commands and native Windows `%TEMP%` screenshots.

Back-project the colour-segmented plane boundaries against adjacent calibrated
full cubes. Coordinates are accepted only when multiple views agree. Restore
the Mojang textures and pixel-compare the vanilla client and Cinnabar from the
same fixed cameras. Any disagreement updates the compact template data, not the
state semantics or generic Cross renderer.

## Performance and correctness invariants

- One compact model ref per selected immutable flowerbed template; no runtime
  geometry expansion beyond the existing packed template buffer.
- At most four additive patch groups per normal state; template/model counts
  remain bounded and encoded in the asset manifest.
- Two-sided alpha cutout, correct flower/stem texture selection, exact cardinal
  rotation, face-specific baked lighting, and conservative cave connectivity.
- No diagnostic cube and no full-height crossed plane for any accepted state.
- Asset encode/decode is deterministic; no Mojang texture or diagnostic pack is
  committed.

## Verification

1. Registry tests prove both names select `FlowerBed`, preserve all 32 typed
   states, and no longer classify `wildflowers` as `Cross`.
2. Compiler tests prove additive growth, cardinal rotation, texture split,
   bounded templates, encode/decode parity, and command-only fail-closed
   behavior until measured.
3. Render tests prove packed refs, lighting, culling, two-sided cutout, and
   direct/MDI address identity.
4. Deterministic BDS gallery tests cover every accepted state and all cardinal
   rotations.
5. Native Bedrock/Cinnabar screenshot evidence proves visual parity at multiple
   angles before the family is marked complete.
