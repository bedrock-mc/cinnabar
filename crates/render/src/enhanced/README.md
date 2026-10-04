# Enhanced rendering extension

Enhanced is currently hard-disabled after GPU faults and system freezes. The
renderer kill switch cannot be overridden by settings, launch flags or camera
components. The implementation below is retained for future investigation and
compiles only with `--features enhanced` (on `render` or `bedrock-client`).

Vanilla is the default. Enhanced is a deliberate non-parity look and never closes
a vanilla parity gate. No shader-pack source is used. The earlier Cinnabar WIP
provided the starting point for the independent WGSL implementation.

Terrain, models and liquids share vanilla RGB lightmap, corner AO, face shade,
biome tint/fog and positional texture selection. Enhanced adds HDR illumination,
shadows, emission and water optics to that shared base; it has no separate scalar
light curve or dark floor. Disabling it preserves the vanilla shader bytes.

Technique references:

- [Microsoft: cascaded shadow maps](https://learn.microsoft.com/en-us/windows/win32/dxtecharts/cascaded-shadow-maps): cascades, texel snapping and filtering.
- [NVIDIA GPU Gems 3, chapter 13](https://developer.nvidia.com/gpugems/gpugems3/part-ii-light-and-shadows/chapter-13-volumetric-light-scattering-post-process): shadowed light scattering.
- [Narkowicz's ACES fit](https://knarkowicz.wordpress.com/2016/01/06/aces-filmic-tone-mapping-curve/): public CC0/MIT rational tone curve. The implementation evaluates the published polynomial coefficients in linear RGB.
- [McGuire and Mara, screen-space ray tracing](https://jcgt.org/published/0003/04/04/): depth-buffer ray intersection. This extension uses a bounded geometric march and binary refinement, not that paper's DDA implementation.
- Bevy 0.18.1 bloom supplies the bloom pyramid. Wind uses analytic sine displacement;
  water uses Schlick Fresnel and Beer-Lambert absorption.

The material-class table is derived from the loaded palette and material IDs.
It does not change mesh generation, geometry arenas or upload budgets. Unknown
materials remain ordinary surfaces. Shared material IDs conservatively combine
classes, so resource packs need native inspection.

Default target: two 1024-square cascades over 96 blocks, half-resolution shafts,
20 SSR steps with five refinement steps. These are quality choices, not measured
performance claims. GPU/CPU diagnostic spans use Bevy's recorder when installed;
Bevy 0.18 GPU diagnostics require Vulkan/DX12 timestamp support; its Metal
recorder reports CPU times only. Use Xcode GPU capture for Metal GPU cost.
Native Metal, DX12 and Vulkan
visual/performance checks remain required.

Camera scope currently assumes the gameplay camera fills its render target.
Custom split-screen viewports need snapshot UV transforms and post-pass bounds.
Actors and world nametags are HDR-compatible, but actors retain vanilla lighting
and do not cast into the terrain cascades. No temporal history, GTAO or TAA is
implemented. SSR cannot reflect offscreen objects or transparent geometry.
