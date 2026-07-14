# Phase 2 vanilla texture vertical-slice report

## Status

The opaque full-cube texture slice is implemented. Normal and deterministic
Front/Back gallery runs pass for 60 seconds at radius 16 with zero errors and
zero missing mappings. The clean no-assets full gate passed, but final review
left Task 8 open on fail-closed deferred materials, the two-second
teleport/full-view remesh gate, and fresh combined RSS/steady-CPU evidence. This
report does not claim full vanilla parity or completion of Phase 2.

Phase 0 remains a conditional go because its authoritative dev-MacBook p99 run is
still outstanding. The results below are from the Windows reference machine and
do not replace that gate.

## Local-only asset provenance

- Mojang source: `bedrock-samples` tag `v1.26.30.32-preview`.
- Source archive SHA-256:
  `12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c`.
- Generated registry SHA-256:
  `df028b265cc4d74b7086075937f7a6a34508c56c014c07b7a47700f32ac9187e`.
- Compiled runtime blob SHA-256:
  `af98e5ddd5532972bf99b9fc3bdd3819bb06b1d8696198f135a9d96ae27ca7ba`.
- Compiled blob: 1,141,588 bytes, 16,913 block visuals, 421 materials,
  388 texture-array layers, five mip levels, and 529,232 texture bytes including
  mips.

The source checkout, archive, unpacked textures, and compiled `.mcbea` remain
under ignored `.local/` paths. No Mojang image, JSON payload, archive, or compiled
asset blob is tracked by Git. The app loads the ignored default blob only when it
exists; `--assets` overrides `RUST_MCBE_ASSETS`, which overrides that default. A
missing blob starts the generated diagnostic checker and prints the exact local
fetch/compile commands rather than downloading content.

## Implemented path

The app resolves block state plus face to a compact material ID while meshing
directly from packed palettes. It emits one eight-byte record per greedy quad.
One global material buffer, one repeat sampler, and one mipmapped 2D texture array
serve the shared chunk pipeline and indirect draw arenas; there are no
per-subchunk Bevy `Mesh` or `StandardMaterial` objects.

The shared sampler uses nearest magnification so the pinned pack's native
16x16 texels remain crisp when enlarged. Minification and transitions between
the independently generated mip levels remain linear to limit distant shimmer.
Anisotropy remains one because WebGPU requires linear magnification whenever
anisotropy is greater than one; a future quality profile may offer that tradeoff
without silently changing the vanilla-pixel presentation.

The user-facing default FOV is 120 degrees horizontally. Bevy stores vertical
FOV, so the camera converts 120 degrees from the primary window's current aspect
ratio and updates the projection after an aspect change. At 16:9 this is about
88.51 degrees vertically, rather than the heavily distorted 120-degree vertical
projection (about 144 degrees horizontally) used by the earlier build.

The original texture slice recorded by this report was deliberately limited to
opaque, axis-aligned full cubes. Cutout and blended blocks, most non-cube models, biome tint, animation,
lighting, sky, fog, and clouds remain later Phase 2 work and resolve to the
diagnostic material where appropriate.

## Verification

At commit `dedca94`, the stale-reply fix was verified with 71 app unit tests,
16 asset integration tests, 10 camera tests, strict client Clippy, formatting,
and `git diff --check`. A reconstructed RED run applies the exact
`b72c53b:app/tests/assets.rs` test blob to its pre-Task-8 parent `266114e`; the
command exits 101 because `app/src/asset_startup.rs` does not yet exist. The
durable evidence is `.worktrees/task8-red-repro/.superpowers/sdd/
task8-red-repro-report.md`.

The clean no-assets full-workspace/Go gate passed at `1604788`: 494 Rust tests,
60 core Go tests, and nine registrygen tests passed; strict all-target Clippy had
zero warnings; formatting, Go vet, and `git diff --check` passed. The worktree
contained no `.local/assets` before or after any command. Its 236 tracked files
contain zero `.png`, `.tga`, `.zip`, or `.mcbea` files and zero
`bedrock-samples` paths. The ignored verification report SHA-256 is
`c55d8a3c36c8102524c9b65b39e78816f4aa2deec554a2809ca020d2615870c2`.

The local fetch and compiler reproduced the pinned source hash and the counts and
blob hash above. Audits found zero runtime-hash collisions and zero missing
lookups. All 49 vetted mycelium/huge-mushroom states resolve every face to a
non-diagnostic material. The current full-cube registry has 669 records.

## Live Windows evidence

Machine: AMD Ryzen 5 3600, Radeon RX 570, Windows 10 Pro, 3440x1440 display.
BDS source executable SHA-256:
`10c680f00faffecdfb3743c5a8a71d6c73f176d148173ca19a99b0c80e40a83f`.

The first two rows are normal automated-flight passes at the instrumentation
commit. The final rows use the deterministic runtime-only BDS gallery, fixed
server camera poses, the same blob, and alternating visible gold/diamond
mutations. Every run uses a fresh BDS 1.26.32.2 runtime copy.

| Artifact | Result | p99 frame | Mutation visible | Errors | Resident / visible | Diagnostic quads |
|---|---:|---:|---:|---:|---:|---:|
| `.local/acceptance/20260711T041205Z-47388` | pass, 60.0008 s | 4.1 ms | 17.6009 ms | 0 | 8,979 / 6,600 | 395,183 |
| `.local/acceptance/20260711T041438Z-20996` | pass, 60.0007 s | 4.1 ms | 27.2172 ms | 0 | 9,040 / 6,681 | 397,720 |
| `.local/acceptance/20260711T052706Z-46768` (Front, `65e0da2`) | pass, 60.0013 s | 3.8 ms | 17.5303 ms | 0 | 9,305 / 6,869 | 411,282 |
| `.local/acceptance/20260711T052936Z-18688` (Back, `65e0da2`) | pass, 60.0001 s | 3.9 ms | 17.5489 ms | 0 | 9,490 / 8,136 | 414,249 |
| `.local/acceptance/20260711T054240Z-45652` (Back + sand support, `1604788`) | pass, 60.0011 s | 3.8 ms | 18.8554 ms | 0 | 9,583 / 8,207 | 419,383 |

All five rows report zero missing mappings. The gallery runs' p99 frame time is
3.8--3.9 ms; the final run's maximum frame/decode/mesh times are
47.001/0.8121/0.8387 ms. The gallery deliberately changes hundreds of blocks and
teleports the camera after initial readiness. Its current `max_remesh_ms`
observations are 14.2--15.1 seconds, so the binding teleport/full-view remesh
target of at most two seconds remains open pending a correctly isolated
measurement and any required scheduling fix.

The intermittent 132-event failure was reproduced with reason counters at
`.local/acceptance/20260711T050537Z-17708` on `58bac3d`. All 132 events were
`inactive_sub_chunks`; every network, world decode, malformed, unexpected,
invalid-dimension, request, retry, and mutation-failure counter was zero. These
are valid replies to requests whose columns left the active radius before the
reply arrived. Commit `dedca94` discards that stale traffic without touching the
store, resident/air state, request/retry state, or error counters. Its TDD suite
and independent review passed; all subsequent gallery runs report zero errors.

## Visual inspection

Computer Use discovered and activated the `bedrock-client` window. Its required
snapshot then failed with the exact Windows error:

```text
SetIsBorderRequired failed: No such interface supported (0x80004002)
```

No app input was sent after that observation failure. Passive GDI capture does
not modify app state. The final evidence frames are:

- Front: `.local/acceptance/20260711T052706Z-46768/frame-fixture-front-final.bmp`.
- Back with stable sand support:
  `.local/acceptance/20260711T054240Z-45652/frame-fixture-back-tight2.bmp`.

Together they show stone, dirt, grass, oak planks, coal/iron/diamond ore, sand,
and glass; x/y/z oak-log beams; opposite cube faces; and repeating plank/glass
UVs at near and far distances without stitched-atlas bleeding. Log ends and bark
remain correctly oriented. The glass texture resolves but renders opaque because
blend semantics are not implemented yet. Oak stairs and glass panes remain the
explicit non-full diagnostic cases. The window title confirms the fixed server
poses and released input state; keyboard/mouse capture and clean release were
already verified in the Phase 0 renderer pass.

Large vegetation volumes outside the cleared gallery remain magenta. The audit
shows these are predominantly leaves/cutout and other non-full states, not missing
asset lookups. This is the next Phase 2 render class and no vanilla-parity claim
is made for it here.

## Exhaustive protocol-1001 visual ratchet

The first global coverage gate now inventories the complete generated registry
through the production BREG1003 and MCBEAS04 decoders. The checked baseline binds
1,356 names, 16,913 canonical states, one air state, the exact sorted state
identity at every sequential ID, registry SHA-256
`394c4566f6231297543e0e0a49889931d7349fba1cf390cb1022ff994a363c03`,
the reviewed invisible allowlist, and the exact diagnostic-state ID set. It
rejects missing/duplicate/non-contiguous IDs, registry/blob lookup mismatch,
new diagnostics, arbitrary diagnostic-to-invisible laundering, stale or
uncited invisible entries, and non-canonical baselines. Diagnostic shrinkage is
reported as an exact identity diff.

Generate a reviewed baseline only when deliberately updating the protocol or
accepted diagnostic set:

```powershell
cargo run -p visualcoverage -- baseline `
  --registry crates/assets/data/block-registry-v1001.bin `
  --assets .local/assets/compiled/vanilla-v1001.mcbea `
  --invisible-allowlist crates/assets/data/visual-invisible-v1001.json `
  --out crates/assets/data/visual-coverage-v1001.json
```

The ordinary CI/local ratchet is:

```powershell
cargo run -p visualcoverage -- ratchet `
  --registry crates/assets/data/block-registry-v1001.bin `
  --assets .local/assets/compiled/vanilla-v1001.mcbea `
  --baseline crates/assets/data/visual-coverage-v1001.json `
  --out .local/assets/compiled/visual-coverage.json
```

The historical 2026-07-13 real-pack run compiled all 16,913 visuals and passed
the ratchet with asset SHA-256
`bd6b8ecb73c4032be51d00dda42d8e5e1b0b55333d276b5cbfa001cb46d0abba`.
It reported 7,722 diagnostics including air and zero diagnostics for lava, vine,
glow lichen, sculk vein, doors, trapdoors, walls, pressure plates, fence gates,
panes, fences, carpets, buttons, or the 48 canonical huge-mushroom states. The
huge-mushroom tranche removed exactly those 48 identities with zero additions
while leaving the 43
legacy flags-zero cube records, 25 transparency-family cubes, and
`minecraft:invisible_bedrock` diagnostic.

The refreshed 2026-07-14 run removes exactly 16 additional diagnostics with
zero additions: `minecraft:black_stained_glass`,
`minecraft:blue_stained_glass`, `minecraft:brown_stained_glass`,
`minecraft:cyan_stained_glass`, `minecraft:gray_stained_glass`,
`minecraft:green_stained_glass`, `minecraft:light_blue_stained_glass`,
`minecraft:light_gray_stained_glass`, `minecraft:lime_stained_glass`,
`minecraft:magenta_stained_glass`, `minecraft:orange_stained_glass`,
`minecraft:pink_stained_glass`, `minecraft:purple_stained_glass`,
`minecraft:red_stained_glass`, `minecraft:white_stained_glass`, and
`minecraft:yellow_stained_glass`, each at canonical state `{}`. The integrated
blob SHA-256 is
`61025bb3e8e1b9ca0d5e2ec1cd7847433333a20f99948c6193fbb370a0d4900f`,
and the refreshed ratchet is zero-delta at 7,706 diagnostics including air.

These cubes use the checked transparent-cube template semantic and six exact
alpha-blended face materials. Palette-native meshing suppresses shared faces
only when both neighbours carry that semantic and their six-face material
identities match. Different colours retain both boundary faces; full opaque
neighbours hide the glass face without losing their own face; glass remains
cave-open; and the rule crosses all six subchunk boundaries. Education
`hard_*` glass, stained-glass panes, copper grates, slime, all legacy flags-zero
records, and `minecraft:invisible_bedrock` remain excluded.

The subsequent copper-grate run removes exactly eight diagnostics with zero
additions: `minecraft:copper_grate`, `minecraft:exposed_copper_grate`,
`minecraft:weathered_copper_grate`, `minecraft:oxidized_copper_grate`,
`minecraft:waxed_copper_grate`, `minecraft:waxed_exposed_copper_grate`,
`minecraft:waxed_weathered_copper_grate`, and
`minecraft:waxed_oxidized_copper_grate`, each at canonical state `{}`. The
checked transparent-cube trust boundary now accepts either six alpha-blended
materials or six alpha-cutout materials while rejecting mixed classes, both
alpha bits, opaque/diagnostic materials, incompatible flags, and malformed
topology. Waxed variants intentionally share exact face-material IDs with the
matching unwaxed oxidation state.

Palette-native meshing suppresses grate faces only when both checked cubes have
the same exact network value. Identical states therefore emit ten ordinary
cutout model draws, while oxidation and wax boundaries emit twelve even when
their textures alias. This holds in sequential and hashed modes and across all
six subchunk boundaries; opaque/grate asymmetry remains correct, no grate uses
the transparent draw stream, and grate walls remain cave-open. Slime,
stained/hard glass, panes, copper bars/bulbs/doors/trapdoors, unrelated grate
names, legacy flags-zero records, and `minecraft:invisible_bedrock` remain
outside the exact grate admission. The ignored integrated blob SHA-256 is
`20cd1b4301f40736468a3249acf21fdea0544d74fa238d8faae04aaee1af9940`,
and the refreshed ratchet is zero-delta at 7,698 diagnostics including air.

The subsequent chiseled-bookshelf run removes exactly the contiguous 256 IDs
1,605–1,860 with zero additions. The compiler requires the complete canonical
64×4 selector product, exact ID formula, reviewed unit collision/full-face
facts, exact `blocks.json` face map, a two-entry empty/occupied front array, and
static side/top entries. It emits exactly four non-diagnostic source materials,
64 immutable templates, and 704 template quads. All four direction rotations,
the eight representative occupancy masks, slot UV seams, ordinary/front
occlusion, all six cross-subchunk boundaries, cave closure, deterministic
reversed input, and a dense full-subchunk model-stream bound are covered. The
refreshed baseline is zero-delta at 2,570 diagnostics including air. Registry
SHA-256 is
`3e0a67718b6368d8b5f7755e9e49a1241233f21bcea8724a9163febb4f1b1d92`;
the ignored integrated blob SHA-256 is
`df82f3408ee5805bcd536a484b6d0e8831eb972d76225c17eda005695e4d982c`.

The subsequent resin-clump run removes exactly IDs 2,930–2,993 with zero
additions. The registry and compiler require the complete typed 64-mask product,
formula IDs, empty flags and coverage, empty collision, and the exact scalar
`resin_clump` block route to the static `textures/blocks/resin_clump` terrain
path. Native 1.26.33.1 support-removal/readback establishes
down/up/south/west/north/east bit order and mask-0 normalization to 63. The
compiler emits one static alpha-cutout material, 63 templates, and 192 quads;
mask 0 aliases the all-face template. Sequential and hashed meshing cover every
mask, every boundary, cave openness, opaque support visibility, water
composition, and the dense 4,096-reference/24,576-draw-light bound. The
refreshed baseline is zero-delta at 2,506 diagnostics including air. Registry
SHA-256 is
`33a31ec89a04fe638a4f59ab315561c1c0d897e04f2041d5643262d3de56d30c`;
the ignored integrated blob SHA-256 is
`91998c61a9f8c40a72e73e45167d7448e9ad18271b561bc61f8d839584603e19`.
The matching-view Cinnabar two-frame live presentation witness remains open.

The subsequent selector-alias opaque-cube run removes exactly 27 reviewed
compatibility states with zero additions, leaving 2,479 diagnostics including
air. The generator and compiler bind all 38 states across the complete hay,
bone, quartz-block, smooth-quartz, chiseled-quartz, purpur, and TNT products;
malformed state wrappers, incomplete products, descriptor aliases, terrain
arrays, overlay tint, flipbooks, and alpha-bearing source images fail closed.
The X/Y/Z native cap permutation is preserved with a quarter-turn UV flag on
the four non-cap faces for X/Z pillars, while `deprecated` and `explode_bit`
are material-identical static aliases. Sequential/hash rendering covers every
state, all six subchunk boundaries, six-quad dense greedy output, cave closure,
and zero model/transparent/liquid streams. Registry SHA-256 is
`9f67a14d73cf958b53557cc31c601168aa0eb95c5d46dfac1299f8412a0cb74f`;
the ignored integrated blob SHA-256 is
`18a4718d6fd03a66c0eb30e0a28444dcf80159c658cf4f7712e5ff342f7740ca`.
The matching-view Cinnabar two-frame live presentation witness remains open.

The subsequent cactus run removes exactly IDs 13,606-13,621 with zero
additions, leaving 2,463 diagnostics including air. The generator and compiler
require the complete exact `age:int 0..15` product, formula IDs, Primary/Cuboid
ownership, empty flags/coverage, shape-84 collision, and exact static
side/down/up terrain routes. Native 1.26.33.1 evidence fixes visible bounds to
X/Z `1/16..15/16`, full Y height, and the source-column 1..14 side crop. All
ages reuse one six-quad template and three static alpha-cutout materials.
Sequential/hash rendering covers every age, every subchunk boundary, opaque
adjacency, cave openness, additional water, and the dense
4,096-reference/24,576-draw-light bound. Registry SHA-256 is
`23a504f0daa248c717249d0aa247362933ff963754aedd790566fc0516cdcf95`;
the ignored integrated blob SHA-256 is
`ddee460c3bad5d14eb81216dc669389813c6a1a805de398de2b95f56bc87bc7d`.
The matching-view Cinnabar two-frame live presentation witness remains open.

The subsequent cake run removes exactly IDs 14,055-14,061 with zero additions,
leaving 2,456 diagnostics including air. The exact seven-state typed product,
formula IDs, shapes 89-95, literal terrain pairs, and native west-cut direction
fail closed atomically. Seven immutable six-quad templates advance only minimum
X by 1/8 per bite; bite zero uses `cake_side` on west and bites one through six
use `cake_inner`. Sequential/hash rendering covers every bite, every subchunk
boundary, opaque adjacency, cave openness, additional water, and the dense
4,096-reference/24,576-draw-light bound. Registry SHA-256 is
`050cf1e79f9505cfcb240b1eb6627df95451e062e77b368b6d2700c21e68c3e6`;
the ignored integrated blob SHA-256 is
`e800994b4bb39e1afc3e77207b510998289b4be7684eb4ac38a0aea677931e94`.
The matching-view Cinnabar two-frame live presentation witness remains open.

The reviewed baseline cumulatively records the already-landed
door/trapdoor/wall removals plus the pressure-plate, fence-gate, pane/fence,
carpet, button, huge-mushroom, glow-lichen/sculk-vein, and ordinary
stained-glass, copper-grate, chiseled-bookshelf, resin-clump, selector-alias
opaque-cube, cactus, and cake tranches,
rather than attributing all 7,350 removed IDs to one
feature. This is a regression baseline, not a parity claim: each remaining
family must reduce that exact set, and the final strict gate still requires zero
non-air diagnostics, 67 exact-state GPU gallery pages, and the separate
block-entity manifest. The local JSON report and compiled Mojang-derived blob
remain ignored; only the generated non-Mojang registry metadata and
deterministic coverage baseline are tracked.

## Remaining work

1. Close Task 8's three Important review findings and rerun the live gate.
2. Add cutout-cube leaves, then decode biome palettes and apply grass/foliage/
   water tinting. These are the next largest visual improvements.
3. Continue the rest of Phase 2: static/non-cube models, blend/water, flipbooks,
   client lighting, sky, fog, and clouds.
