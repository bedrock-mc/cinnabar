# Phase 2 shelf source-authority audit

Date: 2026-07-14

Protocol: 1001

Status: registry/state contract complete; render geometry and UV authority
blocked. Shelf states remain diagnostic.

No Mojang asset, screenshot, archive payload, or extracted binary is tracked by
this manifest. No archive or executable was unpacked, decompiled, disassembled,
or reverse engineered.

## Audited legitimate sources

The installed Microsoft Store Bedrock package was inspected in place:

- package: `Microsoft.MinecraftUWP`;
- package version: `1.26.3301.0`;
- install root:
  `C:\Program Files\WindowsApps\Microsoft.MinecraftUWP_1.26.3301.0_x64__8wekyb3d8bbwe`;
- `appxmanifest.xml` SHA-256:
  `aeecfc787ca27f4c5d969bd385ce93dae580fbdb4680c61d5f1c1c981297088b`.

The installed `experimental_vanilla_shapes` behavior pack is explicit text
data, not extracted binary data:

| Installed relative path | SHA-256 |
| --- | --- |
| `data/behavior_packs/experimental_vanilla_shapes/manifest.json` | `1758855f0d195f097270c72d49ae6add69889f9ec61ae47c6afb3726405b9f4d` |
| `data/behavior_packs/experimental_vanilla_shapes/contents.json` | `fff1f48f680fc9e2ddcf43e09b485441c4b23b69bbe8eb32e35011175037cf3a` |
| `shapes/shelf_facing_south.json` | `76ccb9b8f1d6dabc90e0c524c88214fb25c8641e6131a5fae611f789a52b2219` |
| `shapes/shelf_facing_west.json` | `092b8fccd6d10c767ab9a317fef377102ba66ff827f1b8af40a1cd8d7e5ea12e` |
| `shapes/shelf_facing_north.json` | `323b9f95af134abfb1af4cf523ff29ed8ee64191a399eb86d258e00335624f99` |
| `shapes/shelf_facing_east.json` | `7dea3bc03c87ccba004d872ec6c1b1f3f9fa9a509b6abffbbb6d8fb3395b1b56` |

Each directional file is format `1.21.110` and contains only a
`minecraft:voxel_shape` with three boxes. The files contain no render
component, render model, face assignment, UV coordinates, texture projection,
or material mapping. They are therefore useful collision/selection evidence,
but not visible geometry or UV authority.

The installed versioned vanilla resource pack was also inspected in place:

| Installed relative path | SHA-256 |
| --- | --- |
| `data/resource_packs/vanilla_1.21.110/manifest.json` | `a6767e4b05e0994dcd144991d8c485f61afd9fce079eedffdba071602b1a159f` |
| `data/resource_packs/vanilla_1.21.110/blocks.json` | `39250f21fdad10f489b21859e60fa619fe0cbf4e69d5ac8ba3e1116e88d845c2` |
| `data/resource_packs/vanilla_1.21.110/textures/terrain_texture.json` | `bcc70abbe70e34169d4b70ae17541b964b7ef9c8c77c0f3ea376fb257d498d16` |
| `data/resource_packs/vanilla_1.21.110/textures/blocks/oak_shelf.texture_set.json` | `d7b2d5739c5164cc5c76e55476aed9f4ff8e058a2f289d17fa9a2064a5e752c8` |

The manifest version is `1.21.110`. These files establish shelf texture keys,
paths, and material texture-set metadata only. A text search across the full
installed `data` tree found no shelf render geometry or UV definition. Binary
executables and `.brarchive` files were deliberately not inspected beyond
their filenames.

The ignored `bedrock-samples` resource pack pinned to
`v1.26.30.32-preview` was audited as a second legitimate Mojang source. Its
`blocks.json`, `terrain_texture.json`, shelf PNG/MERS files, and texture-set
JSON likewise establish texture routing/material inputs but contain no shelf
render model or UV mapping. BDS data supplies recipes, catalog/text data, and
server-minimal resource references, not the missing render contract.

## Exact registry result

The exact twelve expected names are acacia, bamboo, birch, cherry, crimson,
dark-oak, jungle, mangrove, oak, pale-oak, spruce, and warped shelf. Each name
has exactly 32 canonical states:

- `minecraft:cardinal_direction:string` = south, west, north, or east;
- `powered_bit:byte` = 0 or 1;
- `powered_shelf_type:int` = 0 through 3.

Registry generation now validates the whole shelf inventory before validating
individual names: exactly 384 `_shelf` records, exactly the twelve-name set,
and exactly 32 records per name. Removing one complete expected family or
replacing it with an unexpected shelf family fails closed. Per-state typed
selectors, formula IDs, projections, role/flags, and pinned voxel/collision
facts remain exact and deterministic.

Generated registry hashes are:

| Artifact | SHA-256 |
| --- | --- |
| `block-registry-v1001.bin` | `922d287a9a195543644542e84f0d948264bff6cd48b8430024107245fa2e1b73` |
| `block-light-registry-v1001.bin` | `60ccea35b44bc88c3b86d25c7778fde3e617f6c1bad737a9af5c239a36e69e95` |

## Honest render and coverage result

Collision/voxel bounds are not used as visible mesh or UV authority. The
speculative shelf compiler promotion, render templates, render-stream tests,
and 384-state coverage removal were reverted.

Two production pack compilations without shelf promotion were byte-identical:

- ignored `MCBEAS05` SHA-256:
  `4880fd066be66983a08ba93cdcde60a71b9f61beb705e8b4160705d69c15d14e`;
- compiled materials: 658;
- texture layers: 939.

The refreshed baseline is bound to the new registry classification but retains
all 384 shelf states as diagnostic. Global diagnostics remain 2,400, not 2,016.
`visual-coverage-v1001.json` SHA-256 is
`bf403468ab25b19bc29053559b8cbf76b015b9696ff3ef771d1ad5108ffdf1a0`.

## Missing authority required to resume

Visual promotion requires a legitimate, version-matched source that explicitly
defines the shelf's visible cuboids/planes and per-face UV mapping, or a
reviewed native-reference procedure that establishes those facts without
deriving them from collision. Texture presence, voxel/collision boxes, and a
replica implementation are insufficient. Until that authority exists, the
384 states must remain diagnostic and this research branch must not be
integrated as a completed rendering tranche.
