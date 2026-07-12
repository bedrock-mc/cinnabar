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

Normal `growth=0..3` initially uses the exact Mojang flowerbed coordinates and
UVs, then is accepted only after native Bedrock image comparison. Command-only
`growth=4..7` is not clamped, wrapped, or guessed. A native measurement gallery
determines whether Bedrock aliases or adds layouts; the compiled family covers
those states only after that evidence is recorded.

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
