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
