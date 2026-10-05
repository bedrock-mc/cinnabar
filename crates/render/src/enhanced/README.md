# Enhanced rendering extension

Enhanced is experimental and remains disabled in default builds after GPU faults
and system freezes. Build `bedrock-client` with `--features enhanced` to try it;
the renderer stays opt-in through settings or `--render-mode enhanced`.

Vanilla is the default. Enhanced is a deliberate non-parity look and never closes
a vanilla parity gate. No shader-pack source is used. The earlier Cinnabar WIP
provided the starting point for the independent WGSL implementation.

Terrain, models and liquids share vanilla RGB lightmap, corner AO, face shade,
biome tint/fog and positional texture selection. Enhanced adds HDR illumination,
shadows, emission and water optics to that shared base; it has no separate scalar
light curve or dark floor. Disabling it preserves the vanilla shader bytes.

Enhanced applies a bounded Cook-Torrance response for direct sun/moon light and
keeps the vanilla lightmap as the indirect diffuse term. Every albedo atlas
revision also builds linear normal and MER (metalness, emissive, roughness)
layers from the Bedrock tiles. Those layers are uploaded once and sampled by
the enhanced block shader. An optional authored pack can replace those
derived layers per texture without changing the shader interface. Water, lava and
foliage retain class-specific material defaults.

## Authored 512x PBR packs

Set `CINNABAR_ENHANCED_PBR_DIR` to one extracted pack directory, or to two
directories separated by `;` on Windows (`base;PBR`) when color and PBR files
are separate. The loader searches `assets/minecraft/textures/block` and
`textures/blocks`, accepts PNG files whose bytes are actually JPEG, and
normalizes every matched layer to 512x512. Enhanced reads `<name>.png`,
`<name>_n.png`/`<name>_normal.png`, and `<name>_s.png`/`<name>_mer.png`.
The `_s` map is converted from old PBR smoothness/metalness/emissiveness to
Bedrock MER metalness/emissiveness/roughness; `_mer` is already Bedrock order.
Missing maps use flat defaults; unmatched blocks retain generated
PBR data. Standard mode never loads or binds these pages.

```powershell
$env:CINNABAR_ENHANCED_PBR_DIR = 'C:\packs\XsRealism-512x-main;C:\packs\XsRealism-pbr-main'
make play CLIENT_FEATURES=enhanced RENDER_MODE=enhanced
```

Technique references:

- [Microsoft: cascaded shadow maps](https://learn.microsoft.com/en-us/windows/win32/dxtecharts/cascaded-shadow-maps): cascades, texel snapping and filtering.
- [NVIDIA GPU Gems 3, chapter 13](https://developer.nvidia.com/gpugems/gpugems3/part-ii-light-and-shadows/chapter-13-volumetric-light-scattering-post-process): shadowed light scattering.
- [Narkowicz's ACES fit](https://knarkowicz.wordpress.com/2016/01/06/aces-filmic-tone-mapping-curve/): public CC0/MIT rational tone curve. The implementation evaluates the published polynomial coefficients in linear RGB.
- [Microsoft: Bedrock PBR texture sets](https://learn.microsoft.com/en-us/minecraft/creator/documents/vibrantvisuals/pbroverview?view=minecraft-bedrock-stable): MER, normal and height map semantics.
- [J. Britain: PBR in Minecraft](https://jbritain.net/blog/pbr-in-minecraft): direct sun BRDF plus lightmap diffuse and reflection terms.
- [McGuire and Mara, screen-space ray tracing](https://jcgt.org/published/0003/04/04/): depth-buffer ray intersection. This extension uses a bounded geometric march and binary refinement, not that paper's DDA implementation.
- Bevy 0.18.1 bloom supplies the bloom pyramid. Wind uses analytic sine displacement;
  water uses Schlick Fresnel and Beer-Lambert absorption.

The material-class table is derived from the loaded palette and material IDs.
It does not change mesh generation, geometry arenas or upload budgets. Unknown
materials remain ordinary surfaces. Shared material IDs conservatively combine
classes, so resource packs need native inspection.

Default target: two 1024-square cascades over 96 blocks, half-resolution shafts,
20 SSR steps with five refinement steps, and one bounded BRDF evaluation per
surface. These are quality choices, not measured performance claims. GPU/CPU diagnostic spans use Bevy's recorder when installed;
Bevy 0.18 GPU diagnostics require Vulkan/DX12 timestamp support; its Metal
recorder reports CPU times only. Use Xcode GPU capture for Metal GPU cost.
Native Metal, DX12 and Vulkan
visual/performance checks remain required.

Camera scope currently assumes the gameplay camera fills its render target.
Custom split-screen viewports need snapshot UV transforms and post-pass bounds.
Actors and world nametags are HDR-compatible, but actors retain vanilla lighting
and do not cast into the terrain cascades. Enhanced cameras use Bevy's temporal
history/TAA after the world post pass; motion-vector coverage for custom
transparent effects remains limited. GTAO is not implemented. SSR cannot reflect
offscreen objects or transparent geometry.
