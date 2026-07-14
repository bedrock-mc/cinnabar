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
`fda4b40335c24b0019049ce572668b03f8ddb9a705de88abb4d724aa7ff81106`,
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

The refreshed 2026-07-13 real-pack run compiled all 16,913 visuals and passed
the ratchet with asset SHA-256
`0323bd2da3cdfffd9afffc3cd525383ed7bc43dc52f19e1b35c232d2ef3db325`.
It reports 7,954 diagnostics including air and zero diagnostics for lava, vine,
doors, trapdoors, walls, pressure plates, fence gates, carpets, or buttons. The
reviewed baseline refresh
cumulatively records the already-landed door/trapdoor/wall removals plus the
pressure-plate, fence-gate, carpet, and button tranches, rather than attributing
all 6,987 removed IDs to one feature. This is a regression baseline, not a parity
claim: each remaining
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
