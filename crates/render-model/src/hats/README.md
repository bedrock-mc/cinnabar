# Cosmetics

Open `bunny_ears.bbmodel` in Blockbench with `bunny_ears.png` beside it.
Open `crown.bbmodel` with `crown.png` beside it for the rigid gold crown.
Open `devil_horns.bbmodel` for red stepped horns attached directly to the head;
its original palette texture is also embedded in the project.
All projects and textures are original Cinnabar artwork under the repository license.
After editing, run `python3 crates/render-model/src/hats/export.py` to regenerate
`bunny_ears.geo.json`, the runtime export. Add `crown` or `devil_horns` to export the other headwear.
Tests check the exports against their editable projects.

Each ear is a connected, shallow extrusion of a stepped pixel silhouette.
The ears sit directly on the head; there is no headband. Flat polygon normals
keep the faces crisp. Upper-half rings follow a chain of joints, with shared ring positions across adjacent faces. There are no internal
caps, gaps or separate rigid pieces at the joints. The lower half stays upright.
Keep each joint pivot aligned with its surface ring and parents before children.
Bake mesh transforms and group rotations into the surface before exporting.

For bunny ears, the first four texture pixels are black, highlight, pink and light-pink swatches.
Face UVs select these swatches in discrete pixel patches; keep the texture unfiltered.

Open `dragon_wings.bbmodel` for the sculpted back attachment. Export it with
`python3 crates/render-model/src/hats/export.py dragon_wings`. The palette is
embedded and is also available in `dragon_wings.png`. The closed membranes and
raised supports are one connected surface per wing.

`dragon_wings.animation.json` supplies one looping cosmetic rotation clip, sampled
linearly in degrees, plus a `tip_flex` profile. Rotations use the authored mesh
axes (X, then Y, then Z); the renderer reflects X with the geometry. Each flex
joint specifies its outward X direction, a rigid starting span, bend length,
base angle, oscillation amplitude and phase delay. Distances are model pixels.
Admission derives a shared joint at each authored X coordinate beyond the rigid
span. All coincident vertices keep the same transform, including thick supports;
only bone poses change during playback. The profile is a Cinnabar cosmetic
extension, separate from Bedrock actor animation behavior. The world and HUD
share a caller-owned looping phase. Head and back selections remain independent.

Open `shoulder_parrot.bbmodel` for the original pet-slot shoulder parrot. Its
original atlas is embedded and available in `shoulder_parrot.png`. Export with
`python3 crates/render-model/src/hats/export.py shoulder_parrot`. The body-root
attachment sits on the shoulder independently of headwear and back cosmetics.
The head, wings and tail remain separate editable groups. Three one-shot timelines
in the project match `shoulder_parrot.animation.json`: a nearby arrival and flights
to either shoulder. Its `playback` profile plays the intro once, then repeats a
cycle of named clips with a delay before each flight. Delays hold the next clip's
first pose. All clip transitions must have matching joint transforms. Positions
are authored model pixels; rotations are degrees. Static gallery artwork uses the
first cycle pose, while world and character previews share caller-owned time.
Exporting this project also updates its animation channels, preserving the playback profile.

## Emissive items

`render_flags.emissive: true` (optional, default false) draws the item fullbright in the
world. Admission derives a world raster from the texture: texels with alpha of at least 128
keep their colour with alpha 0, which the actor's emissive-alpha material draws without world
lighting; all other texels are discarded. Pure black is lifted to `(1, 1, 1)` so it still
draws. Gallery thumbnails and HUD or wardrobe previews use the authored texture unchanged.

## Particle emitters

An animation file may add a top-level `particles` array of at most eight entries:

```json
{"bone": "nozzle_left", "offset": [0, -2.5, 0], "sprites": [0, 16], "effect": {"format_version": "1.10.0", "particle_effect": {...}}}
```

- `bone` names a bone in the item's geometry.
- `offset` is in model pixels from that bone's pivot, on the authored geometry axes (the same
  axes as `.geo.json` cube origins); each component is finite and within ±64. The renderer
  mirrors X with the geometry, so the emitter sits exactly where a cube corner at
  `pivot + offset` renders, and it turns with the bone's rest rotation and animation.
- `effect` is a complete Bedrock particle effect object with a string
  `particle_effect.description.identifier`. Admission replaces that identifier with
  `cinnabar_cosmetic:<first 16 hex digits of the animation SHA-256>_<index>`, so revisions never
  collide with each other or with pack effects. Use a looping emitter lifetime; finished
  emitters restart while the item remains drawn. Textures resolve against the runtime particle
  atlas, for example `textures/particle/particles`.
- An effect whose texture is `atlas.cosmetic` draws the item's own art instead: `sprites` gives
  the top-left texel `[u, v]` of a 16x16 region of the item texture, which must fit inside it.
  Its UVs and flipbooks address that region with `texture_width` and `texture_height` set to 16.
  `sprites` is required with `atlas.cosmetic` and rejected with any other texture.

The emitter basis is the posed bone's axes in world space, including player scale. Any
malformed entry rejects the whole item. Emitters run only for world-visible attachments, with at
most 128 alive at once (sixteen players with eight each); previews never emit.

Ten cube projects fill every slot: `studio_headphones`, `halo`, `pixel_shades`, `kitsune_mask`,
`morpho_wings`, `ember_jetpack`, `fox_tail`, `knit_scarf`, `bee_buddy` and `amethyst_orbit`.
Each `.bbmodel` embeds its original 64x64 texture, also saved beside it as a PNG; items with
particles keep their 16x16 sprite sheet in the texture's bottom-right corner. Export each
with `python3 crates/render-model/src/hats/export.py <name>`. Cube rotations and pivots are
exported, faces without a texture are omitted, and two-sided cards are thin cubes so their
faces never share a plane. The `animation.json` files keep their `playback` and `particles`
fields when clips are re-exported. Wings, tail, scarf and tassel use looping clips; the bee and
the amethyst shards circle on looping position clips; the pixel shades drop in once and then
lift and drop back after each delay. The halo and amethyst orbit are `emissive`.
Rotation keys interpolate along the shortest turn, so a continuous spin is authored as keys
that wrap from +180 to -180 degrees a few microseconds apart.
