# Cosmetic rendering and fixtures

The API serves production cosmetics. The client downloads, verifies, caches and
renders them; it does not bundle the catalog artwork.

Editable Blockbench projects, textures, animations, export tooling and the full
catalog belong in [the private cosmetics repository](https://github.com/cinnabar-mc/cinnabar-cosmetics).
That repository checks project/export consistency and packaged asset digests in CI.
Adding an item with supported features must not require a client change.

The geometry, textures and fixture catalog retained here are stable regression
inputs for client behavior. They are not synchronized with the live catalog. Keep
fixtures limited to the features under test; test emission, lighting and playback
with focused fixtures rather than copying each new catalog item.

To check the entire cosmetics catalog with the production client validator:

```sh
CINNABAR_COSMETICS_CHECKOUT=/path/to/cinnabar-cosmetics cargo test -p render-model api_catalog_assets_admit_through_client_validation -- --nocapture
```

Without that checkout the test reports a named missing fixture. An explicitly
configured checkout must exist and its catalog assets must all validate. Run this
compatibility check when changing the asset format or qualifying a catalog release.

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

## Ground companions

A pet animation can declare `companion` instead of `playback`, `cloth`, `wind` or
`tip_flex`. Its `idle`, `sit`, `trot` and `gallop` fields name looping clips;
`trot_stride` and `gallop_stride` specify blocks travelled per cycle. Optional
`tricks` names up to four one-shot clips. Every declared animation must be used.
Root placement is independent of the owner's animated attachment bone. Author the
model with its feet at Y=0 and retain the descriptor's root bone in its geometry.

Locomotion poses also supply per-bone clearance sections, including protruding
muzzles and tails, independently of the smaller ground-support footprint. These
sections retain their bone centres and turn with the companion.
The caller supplies real collision-shape queries. Followers accelerate toward a
rear-side slot projected onto clear support, steer around obstructions, and commit
to checked hop paths and landings up to one block high,
and settle into a sit. Gait phase follows travelled distance. Tricks play during
extended idle periods and cancel when the owner moves away. Ordinary blockage
never teleports the companion; owner teleports reset only onto checked support. Water, flight and animation-event
particle bursts remain incomplete; ordinary bone emitters work with the posed pet.

## Ground contact effects

An aura item's emitters may declare `trigger: "step"` or `trigger: "landing"`.
Such items contain only ground-triggered emitters, with gallery-only geometry and
no cloth, companion or animation controller. Emitters must use an instant rate
and a one-shot lifetime with numeric active time from 0.05 to 1 second; they never
restart while standing. Existing emitters default to `continuous`.

Ground contacts use completed local physics samples or remote interpolated feet,
including first-person wearers. Distance triggers alternate sides; landing waits
for supported feet after descent. World-space bursts finish after unequipping.
Teleport, dimension, session and actor-lifetime changes reset contact history.
A decal requires continuous coplanar collision support; uneven steps and ledges
suppress it. Ground effects yield when the particle table is full.

Optional `sprite_side` selects a square 16, 32, 64 or 128 texel region (default 16).
UVs address that square, including transparent padding. The dynamic atlas retains
all authored texels and pins the complete region until its last emitter drains.
Use `emitter_transform_xz` for ground planes that retain the contact heading.
