# Enhanced rendering extension

Enhanced is experimental and remains disabled in default builds after GPU faults
and system freezes. Build `bedrock-client` with `--features enhanced` to try it;
the renderer stays opt-in through settings or `--render-mode enhanced`.

Vanilla is the default. Enhanced is a deliberate non-parity look and never closes
a vanilla parity gate. No shader-pack source is used. The earlier Cinnabar WIP
provided the starting point for the independent WGSL implementation.

Enhanced uses raw block/skylight samples with separate HDR directional light,
occluded spatial sky/bounce light and bounded local block lights. Biome tint,
corner AO, geometry and positional textures still come from the shared world data.
Vanilla retains its RGB lightmap and face shading. Server-authored unlit actors
intentionally retain their unlit appearance.

Sun-shadow receiver offsets and depth bias use fixed world distances. Each filtered
depth comparison evaluates the geometric receiver plane at the stored texel centre;
directional rasterization adds no slope bias. Blocker statistics interpolate separately
from visibility, so cascade resolution cannot displace receivers or change their minimum
filter width. Current moving-camera visual acceptance remains incomplete.

Lamp shadow candidates use world distance independently of camera orientation. Each
retained emitter carries its receiver history across atlas lane changes; replacement
invalidates only its own lane. Shadow history advances with submitted frames even
when temporal image anti-aliasing is disabled. Native nearby beacon acceptance is open.

Enhanced applies a bounded Cook-Torrance response for direct sun/moon light and
uses visibility-aware resident-grid sky and one diffuse bounce. Solar, lunar
and local sources share calibrated internal irradiance units; these are not lux.
Authored block layers use linear albedo, tangent normals, roughness, Fresnel reflectance,
metal identity, emissive strength, AO, height, subsurface and porosity data when present.
Water and foliage retain their separate transport. Missing maps have neutral defaults.

Diffuse sky transport includes cloud-scattered radiance in the existing environment
layer, independently of the visible atmosphere LUT. Motion and wind refresh this
layer at preset intervals; weather and source changes invalidate it immediately.
Successful publications schedule bounded indirect relighting with its existing blend.

## Authored PBR packs

The optional ignored `.local/enhanced-pbr.json` selects extracted packs relative to its
own directory. Color/material companions share an explicit group; maps never cross
unrelated pack ownership. `CINNABAR_ENHANCED_PBR_DIR` overrides this configuration.
Bedrock `.texture_set.json` resolves normal or height and MER/MERS textures/constants.
Java LabPBR packs declare `lab-pbr/1.3` through their format metadata or local configuration;
unknown `_s` formats are not guessed. DirectX normals follow increasing image V, with
explicit conversion for OpenGL inputs. Java filenames map to matching Bedrock materials. The local XsRealism companion
has no verified channel declaration; its ambiguous specular/normal maps are skipped,
while its color and standalone height maps remain usable. Faithful declares LabPBR.

```
{"packs":[
  {"path":"packs/base","group":"material-set"},
  {"path":"packs/materials","group":"material-set","format":"lab-pbr/1.3"}
]}
```

Native maps that fit the loader's `LOW_PBR_TILE_SIZE` share a compact page; larger maps
use the shared high-resolution target. Animated water/lava avoid high-page upscaling.
An optional `complete_material` configuration prefers matched complete sets over color-only
overrides. The audit distinguishes vanilla-style colors, authored materials, native source
resolution and photorealistic coverage. Resizing never counts as authored detail.
Authored frame order applies equally to color and
companion maps; independent authored frame counts and durations are sampled into the
carrier animation slots, so exact custom animation timing remains incomplete. Albedo is filtered in linear light with alpha coverage preservation;
normal variance increases filtered roughness. AO and height share the packed normal layer,
so materials need no additional sampled height texture. A content-addressed local cache
avoids re-decoding unchanged inputs. Optional maps decode on a background worker only
after Enhanced is selected; Vanilla-only sessions never load them. Server overlays
retain their own remapped references. Authored-only GPU replacements reuse carrier buffers
and upload at most 8 MiB per render frame, publishing all maps together after completion.
Texture allocation latency and full server-generation reuploads remain separate limits.

The ignored `.local/enhanced-pbr-audit.json` lists each alias's applied color, normal,
material, height, AO and subsurface maps, missing sources, errors and carrier fallbacks.
Cached sessions retain the matching provenance report without decoding maps again;
stale cache/report pairs decode again. A failed pack match still produces the report. Pixel normal
variation broadens unresolved specular highlights, and animated LabPBR metal IDs and
porosity/subsurface branches select an authored frame rather than interpolating categories.

Close opaque surfaces use twelve bounded parallax steps and five directional relief
visibility samples only for authored varying height. Relief fades for subpixel footprints,
grazing angles and distance; cutouts, translucent surfaces and animations keep original
coverage. Relief visibility fades continuously at blockers. UV derivatives give
rotated/mirrored cube and model normals the proper basis.

Technique references:

- [Microsoft: cascaded shadow maps](https://learn.microsoft.com/en-us/windows/win32/dxtecharts/cascaded-shadow-maps): cascades, texel snapping and filtering.
- [NVIDIA: PCSS](https://developer.download.nvidia.com/shaderlibrary/docs/shadow_PCSS.pdf): blocker search and contact-hardening shadows; local emitter kernels select cube faces per ray.
- [NVIDIA GPU Gems 3, chapter 13](https://developer.nvidia.com/gpugems/gpugems3/part-ii-light-and-shadows/chapter-13-volumetric-light-scattering-post-process): shadowed light scattering.
- [Filament lighting and exposure](https://google.github.io/filament/main/filament.html): consistent radiance, irradiance and exposure conventions. Enhanced uses its own hue-preserving luminance shoulder, with a near-linear dark response and smooth highlight gamut compression.
- [Microsoft: Bedrock PBR texture sets](https://learn.microsoft.com/en-us/minecraft/creator/documents/vibrantvisuals/pbroverview?view=minecraft-bedrock-stable): MER, normal and height map semantics.
- [J. Britain: PBR in Minecraft](https://jbritain.net/blog/pbr-in-minecraft): direct sun BRDF plus lightmap diffuse and reflection terms.
- [McGuire and Mara, screen-space ray tracing](https://jcgt.org/published/0003/04/04/): depth-buffer ray intersection. This extension uses a bounded geometric march and binary refinement, not that paper's DDA implementation.
- Bevy 0.18.1 bloom supplies the bloom pyramid. Wind uses analytic sine displacement;
  water uses dielectric Fresnel and Beer-Lambert absorption.
- [shaderLABS LabPBR standard](https://shaderlabs.org/wiki/LabPBR_Material_Standard): exact channel and conductor semantics.
- [Nubis](https://www.guerrilla-games.com/read/nubis-authoring-real-time-volumetric-cloudscapes-with-the-decima-engine): tileable cloud density and weather shapes.
- [Schneider and Vos cloudscapes presentation](https://www.advances.realtimerendering.com/s2015/The%20Real-time%20Volumetric%20Cloudscapes%20of%20Horizon%20-%20Zero%20Dawn%20-%20ARTR.pdf): stratiform and convective morphology motivate separate height profiles around an isotropic shape field.
- [Irradiance fields](https://research.nvidia.com/publication/2019-05_dynamic-diffuse-global-illumination-ray-traced-irradiance-fields): visibility-aware distance moments; our bounded voxel implementation is an independent one-bounce adaptation.
- [Geometric specular filtering](https://www.jp.square-enix.com/tech/library/pdf/ImprovedGeometricSpecularAA.pdf): derivative variance motivates our bounded normal-variance roughness filter.
- [AMD temporal reconstruction](https://gpuopen.com/manuals/fidelityfx_sdk/techniques/super-resolution-temporal/): transparent composition motivates our opaque/composited radiance reactivity; this renderer uses its own TAA.

The material-class table is derived from the loaded palette and material IDs.
It does not change mesh generation, geometry arenas or upload budgets. Unknown
materials remain ordinary surfaces. Shared material IDs conservatively combine
classes, so resource packs need native inspection.

## Quality budgets

In an Enhanced build, Settings > Video exposes the Enhanced rendering toggle and
Performance, Balanced and Ultra choices. Quality controls are disabled while Enhanced is
off; changes apply to the gameplay camera immediately and persist in `graphics.json`.
Vanilla is the default mode and Balanced is the default Enhanced quality. Ordinary
settings edits preserve both extension choices. Older graphics files use Balanced.

| Preset | Work allocation |
| --- | --- |
| Performance | Quarter-resolution atmospheric effects, fewer cloud/reflection samples, smaller shadow maps and lighting batches, slower reflection refresh |
| Balanced | Half-resolution effects, intermediate shadow and sampling budgets |
| Ultra | Half-resolution effects, larger shadow coverage, denser filtering and lighting updates |

The renderer's authoritative sampling/allocation budgets live in [quality.rs](quality.rs),
with lamp shadow limits in [local_lights.rs](local_lights.rs). These are bounded work
choices, not measured frame-rate guarantees. `CINNABAR_ENHANCED_QUALITY` optionally
overrides the saved quality at startup; menu changes can select another quality.

```powershell
$env:CINNABAR_ENHANCED_QUALITY = 'balanced'
make play CLIENT_FEATURES=enhanced RENDER_MODE=enhanced
```

All presets retain stable cascades, full-resolution temporal reconstruction and fixed
shadow visibility history resolution. Lamp depth arrays use their own face
resolution rather than allocating small lamp maps at sun-cascade resolution.
Effect targets and post bindings persist across unchanged frames; quality changes
replace only targets whose sizes change. Reflection filtering and irradiance updates
use bounded batches, retaining existing radiance while replacements settle.
GPU/CPU diagnostic spans use Bevy's recorder when installed;
Bevy 0.18 GPU diagnostics require Vulkan/DX12 timestamp support; its Metal
recorder reports CPU times only. Use Xcode GPU capture for Metal GPU cost.
Native Metal, DX12 and Vulkan
visual/performance checks remain required.

Camera scope currently assumes the gameplay camera fills its render target.
Custom split-screen viewports need snapshot UV transforms and post-pass bounds.
Nested radial shadow coverage is independent of yaw, pitch and FOV; receiver overlap
blends visibility before each handoff. Contact shadows add a bounded local correction.
Directional cascades reserve a common world-space filter border and share receiver
bias and filter width through overlaps. Sunlight visibility resolves beside lamp
visibility at half resolution and reprojects a depth/geometric-normal-matched receiver plane
with a 60ms response. Camera/light cuts, changed caster geometry and asset/dimension
changes reject its history; moving receivers and unmatched edges sample the map directly.
Contact refinement uses the same bilinear depth footprint as hit confidence, with
continuous bracket admission and pixel-based edge fading. Native walking/turning
acceptance remains open.
Actors use the same posed geometry and alpha coverage for sun casting and
receiving. Receiver-plane depth correction follows angled surfaces across shadow
filter samples, keeping the contact displacement bounded. Enhanced uses its own jittered world
history with depth-aware cubic reconstruction, neighbourhood variance clipping
and reactive lighting rejection, before hands/HUD. Submitted actor poses and wind
displacement supply previous-frame motion. Water reprojects its analytic wave displacement
rather than borrowing motion from objects below it. Reactive transparency includes sky
pixels and depth-compatible edge neighbours. There is no general material motion layer;
glass and particles still rely on camera reprojection and radiance reactivity, so fast
effects may lose history.

GPU exposure trims the lowest/highest 10% of a 64-bin luminance histogram and
adapts asymmetrically without CPU readback. Meter history persists independently of TAA
and image disocclusion, avoiding instantaneous brightness changes when reprojection rejects a frame.
A reduced-resolution GTAO/contact-shadow
and a quality-budgeted cloud pass use depth-aware upsampling and world temporal history.
An alpha-tested camera depth pass writes a private target before AO, preserving
the main pass's independent clear and alpha coverage. Depth casters sample the
same Enhanced albedo cutouts as visible terrain. Forward shading applies AO to
indirect light and contact visibility to direct light, leaving emission unchanged.
Untextured models have zero default emission; authored emitting classes retain
their calibrated radiance. Day exposure also meters highlights; night exposure
has a lower midpoint and bounded gain. Tone mapping preserves low radiance and
compresses highlights through luminance to retain material hue and white detail.
Bloom uses a soft HDR threshold and restrained additive scattering, rather than
the default wide low-frequency boost.

Local lighting admits at most 32 nearby emissive block sources and eight sources
per 32-pixel tile. Direct response uses distance attenuation and surface orientation;
the quality preset admits one, two or four shadowed sources. Residual
propagated block light supplies a fallback. Source metadata, tile admission and
point-shadow coverage are cached; unchanged input avoids rebuilds and buffer uploads.
Finite emitter filtering crosses cube-face seams and widens with blocker separation.
Half-resolution receiver visibility has a 45ms exponential response for continuous casters;
sampled foliage uses anchored 15Hz captures and a 133ms response, with depth-aware
reprojection/upsampling. Camera jitter is removed from receiver motion; depth changes,
moving receivers, source changes and camera cuts reject history even when TAA is disabled.
Forward shading checks full-resolution receiver motion before accepting half-resolution
visibility, keeping moving players from borrowing a neighbouring surface's retained light.
Each shadow owner retains its slot until a challenger exceeds its influence by 25%,
and all four visibility lanes have independent smoothing with shared depth metadata.
Actual owner departure rejects history; motion is never delayed in the visible geometry.

An Overworld sky-view LUT integrates Rayleigh/Mie/ozone scattering, concentrating
samples near the horizon. A persistent 32×32×8 unit-irradiance transfer volume follows
[Hillaire's isotropic multiple-scattering equations](https://github.com/sebh/UnrealEngineSkyAtmosphere/blob/master/Resources/RenderSkyRayMarching.hlsl):
The sphere-averaged second-order source (equation 5) and scattering feedback (equation 7)
give `Psi = L2 / (1 - f)` (equation 10).
Sun zenith, altitude and weather interpolate continuously; the volume does not rebuild
as those inputs change. Solar/lunar irradiance and Moon phase scale the same transfer.
Generation warms only for Enhanced views in bounded batches and all incident-light
publication waits for the complete volume. The spherical absorbing ground and RGB,
isotropic higher orders remain approximations; resident geometry owns ground bounce.
Fixed direction/path quadrature remains approximate at high altitude; transfer queries
above the atmosphere clamp to its top row.
The display-only finite-world extension mirrors rays below the geometric horizon,
preserving all directions above it and all physical probe/reflection directions.
Full-resolution
sun, phased moon and stars preserve thin features; height fog gives aerial
perspective. Enhanced enlarges the visible sun/moon discs by 50%, normalizing their
area so source irradiance and lunar phase remain unchanged. Clouds integrate
self-shadowing and approximate higher scattering, with smooth solar/lunar color through dusk;
a 128-square map caches their terrain shadows. Sky and cloud-shadow maps refresh
only when their accumulated lighting, camera or wind changes exceed bounded tolerances.
Source irradiance and sky-fill changes also invalidate sky radiance. Restrained
night airglow and phased moonlight preserve sky detail without daytime exposure.
Far terrain fades to the same visible sky radiance, and cloud history follows layer motion.
Cloud and celestial rays retain their physical directions.
Native cloud geometry is omitted
only on these opted-in views.

Six 128-square geometry reflection captures warm one face per frame, then refresh
at most one dirty face per 100 ms, with roughness mips and local box projection.
Replacement captures retain displayed radiance and blend toward their filtered targets
with a 60ms response. Interrupted updates continue from the displayed result; settled
faces stop blend work. Initial captures and relocation resets publish immediately.
Unchanged probes enqueue no capture views. Captures reuse the main sky LUT,
exclude post effects and UI, omit duplicate sun-shadow passes, never recursively
sample themselves, and reset after movement or lighting discontinuities. They also
share the parent's directional and lamp maps, cloud shadows and spatial irradiance;
publication waits for the matching parent shadow submission. Outside the parent's
shadow/irradiance coverage they retain a bounded sky fallback. Specular
filtering uses GGX convolution and a split-sum BRDF response. Diffuse scene lighting
uses the separate spatial irradiance grid. Forward reflection
fallbacks read the sky LUT from a separate layer of the same environment array;
complete local captures skip fallback evaluation. Water uses
current-frame SSR/refraction, RGB absorption, dielectric Fresnel/TIR, shallow foam
and analytic caustics. Depth-aware refraction rejects sky and foreground texels;
water scattering requires incident light. Pixel footprints filter small wave slopes
using the same coarse wave as geometry. Missing screen geometry falls back to the probes.
Rough screen reflection footprints reject depth discontinuities before using color mips.

Camera-depth and cascade submissions cache validated resident geometry and indirect
commands. Unchanged command bytes are not uploaded again; view bind groups remain
valid until their bound textures change. These reductions need release measurements
before claiming an FPS improvement.

Additional technique references: [GTAO](https://www.iryoku.com/downloads/Practical-Realtime-Strategies-for-Accurate-Indirect-Occlusion.pdf),
[Hillaire atmosphere](https://github.com/sebh/UnrealEngineSkyAtmosphere),
[Bruneton scattering](https://ebruneton.github.io/precomputed_atmospheric_scattering/),
[Guerrilla Nubis](https://www.guerrilla-games.com/read/nubis-authoring-real-time-volumetric-cloudscapes-with-the-decima-engine),
[Filament exposure](https://google.github.io/filament/main/filament.html).
The Enhanced client builds and renders headlessly on Windows Vulkan at 1920x1080.
Pointer/keyboard preset choices and persistence passed; controller input remains unverified.
Native motion acceptance, including stationary flashes and streamed edits, remains pending.
Approximate higher-order scattering, transparent reactivity,
finite reflection coverage and full viewport support remain incomplete. Release
frame-time and streaming acceptance remain open; see `plan.md`.

In focused Enhanced gameplay, `[` cycles normal lighting, cascade coverage,
shadow visibility and the three raw shadow-depth maps. Magenta means uncovered
geometry or a missing cascade. Diagnostics bypass bloom, exposure and TAA;
returning to normal resets temporal history. `]` cycles local time presets.

Windows defaults to Vulkan; explicit `WGPU_BACKEND` selections are respected.
Enhanced-enabled Windows builds using DX12 discover DXC beside the executable or in the
installed Windows SDK. Without DXC they select Vulkan, because FXC overflows its
stack when optimizing the combined cloud/AO shader. Cloud view-step counts come
from the frame uniform; the sampling budget is unchanged. Native regression
coverage compiles optimized pipelines on the default worker stack in a child process.

## Spatial indirect light and cloud cache

A 48×32×48m field voxelizes retained resident cube/model geometry and applied empty-chunk
coverage, then traces integer-grid rays for sky visibility and authored-color diffuse bounce.
Six irradiance lobes and distance moments interpolate connected air volumes; short segment
checks reject wall leaks. Sky-only lobes are bounded by voxel skylight. Unknown far geometry
contributes gated sky fallback and trusted-skylight visibility for known bounce surfaces. Probes
warm nearest the camera first, with 32 probes × 32 rays per batch; static warm inputs stop
updates. Material, geometry, biome-tint, dimension and lighting changes invalidate/update
only the affected local state. Model triangles carry eight half-block occupancy bits
and a conservative cell normal mask in the existing grid buffer. Partial-cell ray tests
and probe relocation leave open slab/stair space available without doubling the field.
Mixed materials/normals within one cell remain approximate, and secondary actor
transport is incomplete. Toroidal storage preserves overlapping probes while the field scrolls;
exposed cells update first and position/epoch guards reject departed aliases. Lighting refreshes
interpolate radiance over 120ms, retaining current visibility moments. Unchanged geometry and
consumed skylight do not invalidate transport. Local geometry and tint changes retain valid
world probes while a bounded refresh completes; current occupancy still gates wall visibility.
Dimension, asset and nonoverlapping grid cuts reject history, and exposed cells still need warmup.

Cloud noise is a persistent 64³ tileable Perlin/Worley volume, generated once on the GPU and
box-filtered through 3D mips before any view samples it. Weather-dependent shapes, vertical
profiles and erosion modulate cached density. Noise mips preserve shape variance to
avoid losing thin clouds; cached erosion combines three spatial frequencies. View,
self-light and terrain shadows share that density and source irradiance;
cached shadow coordinates project receiver height onto
a plane below the cloud layer. Thin cloud history follows wind and opacity-weighted depth.
Internal shadows and multiple-scattering
approximations give lit tops and darker cores. Solar/lunar contributions blend smoothly,
and cloud radiance receives aerial transmittance at its density centroid. Height fog remains physical while streaming
fade stays at the final 14% horizontal range. Renderer regressions cannot replace current
moving-camera native acceptance or user-owned GPU cost measurements.
