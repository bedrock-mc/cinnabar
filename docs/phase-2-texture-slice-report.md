# Phase 2 vanilla texture vertical-slice report

## Status

The opaque full-cube texture slice is implemented and has passed two consecutive
60-second Windows acceptance runs at radius 16. Task 8 is not closed yet: the
deterministic named-block visual gallery and the final independent review remain
open. This report therefore does not claim full vanilla parity or completion of
Phase 2.

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

The current visible scope is deliberately limited to opaque, axis-aligned full
cubes. Cutout and blended blocks, most non-cube models, biome tint, animation,
lighting, sky, fog, and clouds remain later Phase 2 work and resolve to the
diagnostic material where appropriate.

## Verification

At current commit `a03605e`, the normalization instrumentation was verified with
70 app unit tests, 16 asset integration tests, 10 camera tests, strict client
Clippy, formatting, and `git diff --check`. The original Task 8 RED output and
the prescribed post-implementation/pre-ingestion full-workspace gate do not have
durable artifacts, so detailed-plan Steps 2 and 4 remain open. The complete
current workspace/Go gate will be rerun and recorded before Task 8 closure.

The local fetch and compiler reproduced the pinned source hash and the counts and
blob hash above. Audits found zero runtime-hash collisions and zero missing
lookups. All 49 vetted mycelium/huge-mushroom states resolve every face to a
non-diagnostic material. The current full-cube registry has 669 records.

## Live Windows evidence

Machine: AMD Ryzen 5 3600, Radeon RX 570, Windows 10 Pro, 3440x1440 display.
BDS source executable SHA-256:
`10c680f00faffecdfb3743c5a8a71d6c73f176d148173ca19a99b0c80e40a83f`.

Both passing runs used commit `a03605e`, the blob hash above, a fresh runtime copy
of BDS 1.26.32.2, radius 16, automated flight, and alternating visible
gold/diamond mutations.

| Artifact | Result | p99 frame | Mutation visible | Errors | Resident / visible | Diagnostic quads |
|---|---:|---:|---:|---:|---:|---:|
| `.local/acceptance/20260711T041205Z-47388` | pass, 60.0008 s | 4.1 ms | 17.6009 ms | 0 | 8,979 / 6,600 | 395,183 |
| `.local/acceptance/20260711T041438Z-20996` | pass, 60.0007 s | 4.1 ms | 27.2172 ms | 0 | 9,040 / 6,681 | 397,720 |

The first run's maximum decode/mesh/frame times were 1.6438/3.2234/28.4076 ms;
the second run's were 2.5187/2.1042/26.9586 ms. Both reported zero missing
mappings. Their initial full-view remesh maxima were 4,936.9038 and 7,306.2238
ms, so the later Phase 2 teleport/full-remesh target of at most two seconds is
not claimed by this slice.

An earlier otherwise equivalent run at
`.local/acceptance/20260711T035509Z-45484` failed because the legacy aggregate
counter reached 132 normalization events. Network and world decode counters were
both zero. The run had a much heavier world-stream backlog, but its aggregate-only
log could not identify the source. Commit `a03605e` split every normalization
increment into durable reason counters without changing behavior. The two runs
above then passed consecutively with zero events. No speculative semantic change
was made; a future recurrence will identify the exact path.

## Visual inspection

Computer Use discovered and activated the `bedrock-client` window. Its required
snapshot then failed with the exact Windows error:

```text
SetIsBorderRequired failed: No such interface supported (0x80004002)
```

No app input was sent after that observation failure. A passive GDI capture was
saved at
`.local/acceptance/20260711T041205Z-47388/frame-instrumented.bmp`; it does not
modify app state. The frame shows correctly loaded stone, dirt, ore, log, snow,
and other opaque terrain textures at multiple depths. Large vegetation volumes
and other unsupported shapes remain magenta diagnostic geometry, which matches
the slice's current scope rather than indicating a failed texture-array load.

The landscape frame is not enough to prove the complete Step 6 checklist. A
runtime-only BDS gallery still needs to capture named stone/dirt/grass/planks/
ores/sand/glass cases, all three log axes, opposite cube faces, greedy repetition,
near/far mips, and explicit non-cube fallbacks from fixed camera poses. Glass is
also expected to expose the current lack of blend semantics. Keyboard/mouse
capture and release were verified in the Phase 0 renderer pass, but the focused
texture-gallery pass remains open because Computer Use could not snapshot safely.

## Remaining work

1. Run and record the deterministic local BDS texture gallery, without changing
   or committing the source world or Mojang assets.
2. Complete Task 8's independent blocker-only review and closure commit.
3. Add cutout-cube leaves, then decode biome palettes and apply grass/foliage/
   water tinting. These are the next largest visual improvements.
4. Continue the rest of Phase 2: static/non-cube models, blend/water, flipbooks,
   client lighting, sky, fog, and clouds.
