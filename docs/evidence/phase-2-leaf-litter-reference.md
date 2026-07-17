# Phase 2 leaf-litter render reference

Date: 2026-07-16

## Evidence boundary

No reviewed public Bedrock dataset currently contains the complete
state-to-render recipe. The available evidence establishes only these parts:

1. Protocol/registry sources establish the 32 Bedrock states and their exact
   network identities.
2. Official Mojang Java `26.2` client assets establish the four shared
   footprints, cropped UVs, elevation, and cardinal rotation.
3. A native Minecraft for Windows `1.26.3301.0` fixture was started to examine
   the otherwise undocumented `growth=4..7` states, but the complete controlled
   matrix was not captured and therefore is not final authority.

This is a state and geometry compatibility contract. It is not a claim that a
Java client JAR is a Bedrock render-data dump, and no Mojang payload or native
screenshot is stored in the repository.

## Public Bedrock state authority

The checked protocol-1001 registry combines PMMP BedrockData, PrismarineJS
`minecraft-data`, Dragonfly, and Axolotl/Valentine evidence. All four agree on
the state product:

```text
growth = 0..7
minecraft:cardinal_direction = south | west | north | east
sequential index = direction_index * 8 + growth
```

These sources establish IDs, hashes, canonical state types, and collisionless
semantics. They do not contain vanilla Bedrock mesh positions, UVs, material
flags, or the render meaning of `growth=4..7`.

## Official Mojang geometry and UV authority

The local launcher manifest
`%APPDATA%/.minecraft/versions/26.2/26.2.json` pins the official Mojang client
JAR URL and SHA-1:

- URL: `https://piston-data.mojang.com/v1/objects/2dc72797acbc1b63fc16a11c4ac393605f453754/client.jar`
- Size: `39,193,383` bytes
- SHA-1: `2dc72797acbc1b63fc16a11c4ac393605f453754`
- SHA-256: `40896ee9f1e2bec3c934daac7e93d41e9e3d9c2f8ae0ca366d52ffbfd1afa290`

Reviewed entries and SHA-256 digests:

| JAR entry | SHA-256 |
| --- | --- |
| `assets/minecraft/blockstates/leaf_litter.json` | `a8693a5afc1ee19cdddd4374d779b1ba59416360109029026068421060ee423f` |
| `assets/minecraft/models/block/leaf_litter_1.json` | `6ae48163d59ca1ba2f0143d7c867717a994c7cc90cb350d3e3a55b8246bc3e31` |
| `assets/minecraft/models/block/leaf_litter_2.json` | `92c5c43475d5e2d00ed34e8aa6796f8954267ef6664aaa89a47e5d7803130ebd` |
| `assets/minecraft/models/block/leaf_litter_3.json` | `a3b716febc1d0723a636aecde85314046901c788075097f177e1ba0ab9110c28` |
| `assets/minecraft/models/block/leaf_litter_4.json` | `fe31570f94a24ac10cf3955d401e046462fd638c46107963a32e96fc864e3647` |
| `assets/minecraft/models/block/template_leaf_litter_1.json` | `4fe50ab76971c8c413bec2735a80819d938c9c5e682408cfdfe2176f78fe6418` |
| `assets/minecraft/models/block/template_leaf_litter_2.json` | `32ce807c8dfcd056c320f589d0bf4e7b25dcaeeab1ae3e97888d4b5cd8f55e01` |
| `assets/minecraft/models/block/template_leaf_litter_3.json` | `fec3d76c818e008552587b9cb6d4170ef80a892c97065a9b21a8e15abe50f255` |
| `assets/minecraft/models/block/template_leaf_litter_4.json` | `01f169dc15fe51f8c02735379f222141eaddc23785cd011970955447a0b5779d` |

Those files establish this north-baseline layout in Cinnabar's 1/256-block
positions and 1/4096 UV units:

| Amount | Footprint | Elevation | UV footprint |
| ---: | --- | ---: | --- |
| 1 | `0..128 X`, `0..128 Z` | `4` | `0..2048 U`, `0..2048 V` |
| 2 | `0..128 X`, `0..256 Z` | `4` | `0..2048 U`, `0..4096 V` |
| 3 | amount 2 plus `128..256 X`, `128..256 Z` | `4` | matching cropped quadrants |
| 4 | `0..256 X`, `0..256 Z` | `4` | `0..4096 U`, `0..4096 V` |

The official blockstate rotates north `0`, east `90`, south `180`, and west
`270` degrees. Templates contain both up and down faces; Cinnabar represents
the same visible surface as one two-sided cutout quad per coplanar region.

## Incomplete native Bedrock state matrix

The native fixture used Minecraft for Windows `1.26.3301.0` in Classic Fancy
mode. A quartz platform was placed at Y `90`; leaf litter was placed at Y `91`
with growth advancing across X and direction advancing across Z:

```text
x(growth) = -7 + growth * 2
z(south) = -6
z(west)  = -2
z(north) =  2
z(east)  =  6
```

Every cell was forced with an exact command of this form:

```text
setblock <x> 91 <z> minecraft:leaf_litter ["growth"=<0..7>,"minecraft:cardinal_direction"="<direction>"]
```

Only part of this intended matrix was rebuilt and inspected before this tranche
was deferred. Native Windows Graphics Capture frames were not written into the
repository. The partial observation is insufficient to establish an exact
alias table or rotation contract.

The feature branch currently carries this provisional mapping:

| Bedrock growth | Compiled layout |
| ---: | ---: |
| 0 | quarter |
| 1 | half |
| 2 | half plus opposite quarter |
| 3 | full |
| 4 | full |
| 5 | full |
| 6 | full |
| 7 | full |

The provisional alias table is `[0, 1, 2, 3, 3, 3, 3, 3]`. It must not be
integrated as exact Bedrock behavior until further authoritative data covers
all eight growth values and four directions, including rotations and UVs.

## Material authority

The pinned `bedrock-samples` source supplies the vanilla `leaf_litter` block
texture route and binary-alpha texture. Bedrock Wiki's versioned tint table
lists `minecraft:leaf_litter` under `dry_foliage`, consistent with the native
biome-tinted result and Microsoft's documented dry-foliage client-biome
component. Cinnabar therefore uses alpha cutout plus dry-foliage tint and
rejects malformed, animated, aliased, or partially transparent source inputs.
