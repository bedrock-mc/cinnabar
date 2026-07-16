# Native Legacy Cloud Parity Design

## Goal

Correct Cinnabar's finite cloud renderer to follow the matching Bedrock 1.26.33.1 legacy
`Clouds` path: exact version-matched occupancy, native quality controls, transparent
depth-aware composition, directional lighting, and exact weather colour contributions. Keep
the compact immutable cloud mesh and one-draw architecture where those remain compatible.

## Evidence and authority

The installed matching client at
`Microsoft.MinecraftUWP_1.26.3301.0_x64__8wekyb3d8bbwe` is the visual authority for this
correction. Its `textures/environment/clouds.png` has SHA-256
`f19b2f3a483af3a67568dfed4387c7b59fed215edf1cb02bef0470f2b72982a0`, is 7,880 bytes,
and decodes to 13,356 occupied texels. The currently pinned preview texture differs at
24,175 of 65,536 occupancy coordinates and is therefore not acceptable cloud geometry input.

The matching `cloud_configuration.shared.json` defines the following legacy controls:

| Quality | Grid size | Mesh size | Distance scale | Distance control | Lighting |
| --- | ---: | ---: | ---: | --- | --- |
| Low | 1 | 64 | 2 | enabled | enabled |
| Medium | 2 | 64 | 3 | enabled | enabled |
| High | 3 | 64 | 3 | enabled | enabled |
| Ultra | 4 | 64 | 3 | enabled | enabled |

The matching `Clouds.material.bin` identifies a transparent legacy material and consumes
`CloudColor`, `DistanceControl`, `LightDiffuseColorAndIlluminance`, and
`LightWorldSpaceDirection`. `CloudsForwardPBR` is a separate path and is not silently folded
into this v1 correction. The matching weather configuration supplies rain cloud colour
`[191, 191, 191]` with contribution `0.95` and thunder cloud colour `[30, 30, 30]` with
contribution `0.95`.

## Asset and provenance contract

The exact matching cloud PNG is a local-only input. No Mojang payload, generated atmosphere
blob, native material, screenshot, or extracted binary is committed. The atmosphere compiler
accepts an explicit cloud override path, verifies the exact SHA-256, byte and decoded-dimension
bounds, and fails closed on any mismatch. Sun and moon continue to come from the pinned pack.
The runtime carrier retains the canonical logical cloud source path and its independent encoded
and decoded hashes; an asset schema change is unnecessary unless implementation proves the
existing descriptor cannot express those facts.

Portable build entry points expose the override without embedding a machine path. Local
native-parity builds must be reproducible by passing one documented variable or CLI flag.
Tests use synthetic fixtures, never the Mojang payload.

## Render architecture

The existing periodic packed-quad mesher remains the geometry foundation. Cloud resources are
created once per atmosphere identity: one immutable storage buffer, one identity-cached bind
group, one pipeline family, and one cloud render item. There is no per-frame rebuild, upload,
bind-group creation, per-cell draw, Bevy `Mesh`, or `StandardMaterial`.

The legacy cloud color pass is queued in Bevy's transparent world phase with alpha blending.
It retains reversed-Z depth testing against world depth so terrain and clouds occlude in the
correct order, while the transparent color pass does not write depth. Exact transparent item
sorting is deterministic from the view and cloud bounds. A later explicitly selected
`CloudsForwardPBR` path may add a separate depth prepass; it is not inferred from the legacy
material.

The vertex shader reconstructs a face normal and world position from the packed record. The
fragment shader uses the real atmosphere sun direction and diffuse daylight instead of fixed
top/side/underside multipliers. Weather blends use the exact matching rain/thunder colours and
contributions. Fog and distance control operate on world distance and preserve the existing
absolute-time +X motion of 0.03 blocks per Bedrock tick.

## Quality, grid, and distance semantics

`CloudQuality` and `CloudRenderConfig` carry all four exact native records, with High as the
documented default. The code must not guess what `cloud_mesh_size: 64` means in world units or
assume that `grid_size` directly means an N-by-N count. A focused native matching-view
calibration establishes those semantics before replacing the current coverage math. Until that
evidence exists, configuration is represented and tested but no unsupported mapping is called
parity. Coverage must remain bounded, symmetric across negative coordinates, and large enough
to prevent a visible edge before fog reaches full strength.

## Correctness and acceptance

- The exact cloud override is independently hash- and dimension-validated and its provenance
  appears in the generated report without exposing a machine path.
- Synthetic tests prove override rejection, deterministic encoding, and unchanged sun/moon
  selection.
- Pipeline tests prove transparent phase selection, alpha blend, reversed-Z depth test, no
  legacy color-pass depth writes, correct shader visibility, one draw, and stable resources.
- Shader tests prove exact weather constants, directional response for known light directions,
  bounded alpha, and fog ordering.
- Quality tests prove the exact four native records. Calibrated coverage tests are added only
  after native evidence fixes the world-space interpretation.
- A release BDS run uses temporary GDI screenshots below, above, within, and at grazing angles,
  including period crossings and negative coordinates. Matching native views are the visual
  reference.
- Acceptance requires no cloud-edge pop, no opaque white slabs, no black celestial rectangles,
  one steady cloud draw, zero steady cloud uploads, and compliance with the Phase 2 CPU/RSS and
  teleport publication budgets.
