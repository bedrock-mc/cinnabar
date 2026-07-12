# Phase 2 Block-Family Inventory

Date: 2026-07-11

This is the reproducible planning baseline for Phase 2.6 family generators. It
classifies every protocol-1001 canonical block name and state without treating
collision boxes as visible geometry or UV authority. It contains no Mojang pack
payload.

## Inputs and method

The audit used these pinned inputs:

- PMMP BedrockData `bdb44a48fb6beffb6e9f6864f06d2232eb62b6a3`
  (`6.7.0+bedrock-1.26.30`), including the exact 1,356-name/16,913-state
  protocol-1001 palette;
- PrismarineJS minecraft-data
  `6ec59288287e4045331eaa47ee8fb104278f6b98`, including
  `blockStates.json`, `blocks.json`, and `blockCollisionShapes.json`;
- Dragonfly `b85c56ffea6b306798a935f14cc941c76618be52`;
- Axolotl Stack `6f6806e821a579c183c44d786f76d9b358a2b825`
  and its Valentine overlap catalog; and
- the locally acquired, ignored Mojang 1.26.30 sample resource pack described
  by `assets/vanilla-source.json`.

The final audited `BREG1003` export is 4,692,247 bytes with SHA-256
`3669be82850824af8592276afe864d903495e743b8af81dfcf1d3aa1586231a4`.
It decoded exactly to EOF and reported 1,356 names, 16,913 states, 1,321
Valentine names, 15,845 Valentine states, and attributable gaps of 35 names and
1,068 states.

Reacquire and regenerate the ignored evidence with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass `
  -File scripts/acquire-block-data.ps1

Push-Location tools/registrygen
go test ./...
go run . -out ../../.local/task2/block-registry-v1001.bin `
  -pmmp ../../.local/assets/block-data/pmmp `
  -prismarine ../../.local/assets/block-data/prismarine `
  -valentine-palette ../../crates/protocol/vendor/valentine/bedrock_versions/v1_26_30/src/block_palette.bin `
  -valentine-blocks ../../crates/protocol/vendor/valentine/bedrock_versions/v1_26_30/src/blocks.rs
Pop-Location

Get-FileHash -Algorithm SHA256 `
  .local/task2/block-registry-v1001.bin
```

The inventory parser decoded every bounded `BREG1003` record, grouped records
by canonical name, and joined each name's Prismarine state-ordered collision
shape IDs. It counted typed property domains, empty/unit/multi-box topology,
and direct Mojang `blocks.json` name coverage. The initial comment line in
Mojang reference JSON was excluded before JSON decoding. Counts below must be
regenerated when any pin or registry schema changes.

## FlowerBed normal-state baseline

`minecraft:wildflowers` and `minecraft:pink_petals` now compile Growth 0-3 as
one through four immutable additive ground-patch groups for every preserved
cardinal value: South=0, West=1, North=2, East=3. Each terrain key must expose
exactly the routed flower material at array index 0 and stem material at index
1 in an exact two-entry array; a dedicated terrain accessor rejects static,
missing, and overlong arrays independently of which registry states are being
compiled. The template key contains both material IDs, growth, and orientation.
Growth 4-7 is still an explicit attributable diagnostic and is never clamped,
wrapped, or aliased.

The compact coordinates and UVs are a geometry baseline derived from the four
Mojang Java `flowerbed_1.json` through `flowerbed_4.json` models in the pinned
`MrHakan/mc-mapping-tree` commit
`be56c80939e94f4afd5e63bc40c684af9ff218fb`. Source element coordinates were
rotated as declared by those models, quantized once to the existing 1/256-block
position format, and retain 1/4096 texture-tile UVs. The resulting additive
prefixes contain 7, 10, 17, and 20 two-sided quads, respectively, with all
vertices below 64/256 block height. The pinned `wildflowers.json` blockstate
defines the table as a North-facing baseline: North is identity, East rotates
90 degrees, South 180 degrees, and West 270 degrees. These map to preserved
Bedrock values North=2, East=3, South=0, and West=1. In the packed `(x,z)`
coordinate convention, East=90 is `(256-z,x)` and West=270 is `(z,256-x)`.
This Java-derived table is provisional geometry evidence only; Task 4's pinned
native Bedrock gallery remains the authority for final coordinates, UV
orientation, and command-only Growth 4-7 semantics.

## Exhaustive renderer-work partition

The cube row is deliberately conservative. It assigns all states of a name
with at least one Dragonfly `Solid` state, then removes the known visible
non-unit exceptions described below. Unresolved names stay in the residual;
none are omitted.

| Renderer family | Names | States | Shapes | Empty | Unit cube | Multi-box |
|---|---:|---:|---:|---:|---:|---:|
| Air/invisible/engine-only | 25 | 36 | 2 | 33 | 3 | 0 |
| Cube, Dragonfly-backed after audited exceptions | 366 | 676 | 1 | 0 | 676 | 0 |
| Leaves | 11 | 44 | 1 | 0 | 44 | 0 |
| Cross plants | 60 | 256 | 1 | 256 | 0 | 0 |
| Crops | 11 | 176 | 15 | 156 | 0 | 0 |
| Aquatic cross/fans | 32 | 99 | 1 | 99 | 0 | 0 |
| Liquid | 4 | 64 | 1 | 64 | 0 | 0 |
| Slab | 136 | 272 | 3 | 0 | 136 | 0 |
| Stair | 64 | 512 | 8 | 0 | 0 | 512 |
| Door | 21 | 672 | 1 | 0 | 0 | 0 |
| Trapdoor | 21 | 336 | 6 | 0 | 0 | 0 |
| Pane/bars | 43 | 43 | 1 | 0 | 0 | 0 |
| Fence | 13 | 13 | 1 | 0 | 0 | 0 |
| Fence gate | 12 | 192 | 3 | 96 | 0 | 0 |
| Wall | 32 | 5,184 | 18 | 0 | 0 | 0 |
| Rail | 4 | 46 | 1 | 46 | 0 | 0 |
| Torch | 10 | 60 | 1 | 60 | 0 | 0 |
| Button/lever/pressure plate | 31 | 440 | 1 | 440 | 0 | 0 |
| Chest | 11 | 44 | 1 | 0 | 0 | 0 |
| Sign | 36 | 4,872 | 3 | 2,568 | 0 | 0 |
| Bed | 1 | 16 | 1 | 0 | 0 | 0 |
| Other template/residual | 412 | 2,860 | 119 | 582 | 812 | 54 |
| **Total** | **1,356** | **16,913** | | **4,400 globally** | | |

Walls and signs alone cover 10,056 states. Their cardinality reflects selector
products, not 10,056 distinct hand-authored models.

## Selector requirements

The generated selector representation or the preserved canonical typed state
must cover at least:

- liquid depth: 16 values;
- slab vertical half, with double slabs represented by separate names;
- stair direction (4) and upside-down bit;
- door cardinal direction (4), hinge, open, and upper-half bits;
- trapdoor direction (4), open, and upside-down bits;
- gate cardinal direction, in-wall bit, and open bit;
- four wall connection heights (`none`, `short`, `tall`) and wall-post bit;
- rail direction (10) and rail-data bit;
- torch facing (`unknown`, west, east, north, south, top);
- button facing and pressed bit, eight-valued lever direction, and weighted
  plate signal 0-15;
- standing-sign rotation (16), wall facing, hanging, and attached bits;
- bed direction, head/foot, and occupied bits;
- vegetation ages up to 26, upper-half/tip/hanging/propagule selectors;
- crop growth, including oriented cocoa pod geometry; and
- kelp age, three seagrass variants, coral direction, and fan direction.

The original seven-field `ModelState` is not enough by itself. In particular,
it does not represent namespaced cardinal direction, torch and rail direction,
button/lever state, wall heights/post, sign attachment, bed head/occupied,
door upper-half, or gate in-wall. Family generators must either receive an
expanded typed schema or decode the preserved canonical typed state.

## Collision evidence

Prismarine supplies 342 reusable collision shapes: 12,513 non-empty states,
4,400 empty states, 23 multi-box shape definitions, and at most seven boxes.

- Slabs provide lower, upper, and full shapes.
- Stairs provide eight two-box shapes.
- Trapdoors provide six useful oriented/open shapes.
- Gates provide three shapes; 96 open states are empty.
- Walls provide 18 shapes but simplify every state to one bounding box.
- Doors, fences, panes/bars, chests, and beds each collapse to one simplified
  shape.
- Controls, rails, torches, crossed plants, aquatic plants, and liquids have
  empty collision.

This supports slab/stair bounds and conservative occlusion. It cannot supply
visible walls, fences, panes, chests, signs, controls, plants, or their UVs.

## Classifier pitfalls

Known false positives:

- `_flower` classifies `chorus_flower` as a cross, but it is a cuboid cluster.
- Dragonfly `model.Solid` marks 34 visible non-unit states as cubes: eight
  copper-golem-statue material names with four states each, plus `soul_sand`
  and `mud`.
- `cocoa` belongs to the growth/crop domain but needs an oriented pod template,
  not crossed crop quads.

Known false negatives:

- `iron_bars` and eight copper-bar names are missed by a pane-only suffix rule;
- all walls (32 names/5,184 states), controls (31/440), rails (4/46), torches
  (10/60), and the bed (1/16) are otherwise left unknown;
- `colored_torch_*` does not end in `_torch`;
- most modern flower names do not end in `_flower`; and
- melon and pumpkin stems are absent from the original crop allowlist.

Flags-only cube promotion also strands 43 cube states inside 23 names whose
sibling states are already cubes: one state in each of 16 glazed-terracotta
names, nine states each in `bone_block` and `hay_block`, two states each in
`quartz_block`, `chiseled_quartz_block`, `smooth_quartz`, and `purpur_block`,
and one state in `tnt`. Render family is a name/family decision; occlusion
confidence may remain state-specific.

## Residual topology

The 412-name/2,860-state residual is bounded and attributable:

- 229 names/812 states have full-cube collision. This is only a candidate set:
  it also contains shulker boxes, pistons, chorus flower, azalea, Education
  workstations, spawners, and other visible exceptions.
- 23 names/477 states have entirely empty collision: ground overlays, powder
  snow, portals/gateway, vines/lichen/sculk vein, item frames, banners,
  redstone/tripwire, scaffolding, sea pickle, small dripleaf, frog spawn,
  spore blossom, and related plants.
- Five names/58 states use multi-box collision: `composter`,
  `end_portal_frame`, `hopper`, `cauldron`, and `brewing_stand`.
- 155 names/1,513 states use partial or mixed single boxes. Reusable groups
  include carpets, candles/cakes, anvils, heads, shelves, chains/lanterns/rods,
  amethyst buds, copper-golem statues, farmland/path/snow, campfires, ladders,
  cactus, pots, workstations, eggs, chorus, sculk sensors, redstone devices,
  bell, and conduit.

Dragonfly already exposes useful behavioral/bounding semantics for many of
these groups. Those semantics are procedural inputs, not render geometry or UV
authority.

## Mojang mapping gaps

The pinned pack has 1,231 real `blocks.json` entries and 1,300 terrain keys.
Direct canonical-name lookup covers 1,181 names; 175 require aliases, special
handling, or sourced engine-only treatment:

- 146 residual names, dominated by 119 Education `element_*` names and
  Education workstations/hard-glass blocks;
- 17 hard-glass pane names;
- five colored/underwater torch names;
- five engine-only names without block entries; and
- `grass_block` and `sea_lantern`, which are ordinary alias cases.

Hard glass can reuse reviewed glass/stained-glass aliases. The standard sample
pack has no direct terrain keys for Education elements, underwater TNT,
colored/underwater torches, chemical heat, or material reducer. Zero-diagnostic
coverage needs explicit reviewed aliases or sourced engine-only handling; it
cannot be inferred from collision or `blocks.json`.

## High-impact implementation order

1. Liquids and flipbook animation.
2. Terrestrial/aquatic crossed vegetation and crops.
3. Slabs and stairs.
4. Walls (5,184 states from one connection-aware generator).
5. Signs (4,872 states from a small geometry set).
6. Doors and trapdoors.
7. Controls, rails, and torches.
8. Panes/bars, fences, and gates.
9. Reusable residual templates and individually reviewed full-cube candidates.
