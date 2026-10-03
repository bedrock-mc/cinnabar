# Lifeboat offline terrain investigation

The owner supplied two screenshots and a client log tail. Both screenshots show
working HUD controls and a black first-person hand. Geometry is being published:
the recorded ready snapshot has 1,949 rendered and visible sub-chunks. The tail
attributes 467,150 of 467,200 diagnostic quads to sequential ID 15844, labelled
`minecraft:mushroom_stem` by the checkout's registry.

## What the installed artifacts prove

The optional `installed_lifeboat_carriers_reject_stale_diagnostic_geometry` test
reads installed carriers through this worktree's `.local` symlink. It skips when
those files are absent and never writes them.

At ID 15844, the current pinned world carrier contains an exact cube with a
non-diagnostic material. The older installed `vanilla-v2168.mcbea` contains a
diagnostic visual, identified by inspecting its legacy envelope records. Its
historical registry identifies this slot as a reserved
placeholder, rather than the current registry's mushroom stem. Interpreting that
older carrier through the newer attribution registry can therefore produce the
observed label under the wrong visual. The current decoder rejects the older
carrier schema before startup can bind it; the provenance gate also prevents
foreign registries from reaching gameplay.

This is an offline reproduction of the diagnostic identity mismatch, not a
replay of the server's terrain. No StartGame, chunk payload or session carrier
identity was captured in the supplied tail. A stale carrier/build is a supported
explanation; the exact session cause remains unconfirmed. Do not infer a palette
translation rule from this one runtime ID.

## Packs and lighting

All eleven verified Lifeboat cache archives contain encrypted content metadata.
The cache intentionally stores archive bytes without content keys or download
URLs (`core/packcache/cache.go`). Their decrypted catalogs cannot be reconstructed
offline from those entries. No credentials were read.

Vanilla terrain overrides change material texture references, not vanilla visual
kind or support. They skip diagnostic material zero and failed texture decodes
(`block_overlay.rs`, `override_vanilla_materials`). Such failures do not explain
the diagnostic classification of the known vanilla ID.

The first-person hand samples solved light at the authoritative eye position
(`actor_publication.rs`). Incorrect opaque blocks can consequently darken both
terrain and the hand. The screenshots alone do not establish an atmosphere or
lightmap defect; no speculative lighting adjustment was made.

## Vanilla references

Lens source-backed searches in the reconstructed 1.26.50.26 client identify
`FUN_143197c60` (RVA `0x3197c60`, sequential lookup) and `FUN_143197a70`
(RVA `0x3197a70`, hashed lookup), both with the unknown-ID air fallback. The
owner's reconstruction provides readable corroboration:

- `R:BlockPalette:97-105`: an unknown runtime ID resolves to default air, with a
  palette-disagreement diagnostic. `R:BlockPalette:120-125`: an in-range ID indexes
  the palette directly. Cinnabar retains these lenient semantics.
- `R:BlockPalette:16-19` and `R:BlockPalette:215-220`: palette insertion assigns
  the sequential network index.
- `R:Level:35879-35885`: the world retains the block-network-ID hash mode.
- `R:Actor:18725-18743`: spatial brightness sampling.
  `R:BaseLightTextureImageBuilder:270-304`: sky/block light composition.
- The installed vanilla pack's `blocks.json`, entry `mushroom_stem`, names six
  terrain face keys. `textures/terrain_texture.json:5210-5261` supplies their
  texture routes. Pack content is read at runtime and is not committed here.

`R` denotes the by-owner files under the owner's
`mcsrc-1.26.50/reference/26.30/src/by-owner` reconstruction, not pasted code.

## Changes and next-session evidence

The proven log flood is fixed: `DIAGNOSTIC_GEOMETRY` emits immediately, then at
most every five seconds, retaining the newest pending snapshot. Metrics still
update on every changed resident attribution, and a pending snapshot is emitted
even if geometry stops changing.

The following diagnostics are bounded:

- `START_GAME_BLOCK_IDS`: hash mode, raw block-property count, parsed custom
  block/state counts and skipped definitions, once per prepared session.
- `SESSION_BLOCK_PALETTE`: mode, air ID, visual count, custom internal range,
  whether sequential IDs are remapped, and carrier registry provenance.
- `BLOCK_PALETTE_SAMPLE`: the first sixteen distinct wire IDs per stream,
  their internal IDs, known status, visual support and light properties.
  `UNRESOLVED_BLOCK_ID` names the first eight unresolved wire IDs and air fallback.
- `PACK_TERRAIN_FAILURE`: at most eight failures per compiled stack, with a
  bounded key/path and the catalog, lookup, raster or texture-set failure reason.
- `WORLD_LIGHTING`: every five seconds while a stream exists, immediately after
  session/dimension changes. It prints the eye position and optional solved light,
  dimension/medium, daylight, brightness/effects, fog/sky profile and colors,
  and the darkest/full-light lightmap entries.

If a wire ID resolves to an unexpected known opaque block, the mode, remap and
carrier hash distinguish that from a missing ID. If the palette is coherent,
terrain failures and eye/lightmap/fog state separate pack and lighting failures.
No public-server connection or visual parity gate was completed by this work.
