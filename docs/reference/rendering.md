# Image clarity and sampling

Vanilla rules for the pinned Bedrock version are separated below from evidence that still
needs a matching platform capture. The desktop sampling implementation uses the established
rules; this document does not close the cross-platform visual parity gate.

| Area | Vanilla rules | Evidence limit |
| --- | --- | --- |
| Anti-aliasing control | Video names the slider **Anti-Aliasing**. Values represent positive power-of-two sample counts; `1` is single sampling. Stops come from renderer capabilities. | A universal fixed maximum is incorrect. The inspected capability path can restrict values above four samples. |
| Default sample count | The inspected Windows raster settings request `2` samples and retain that value when supported; otherwise they select the nearest supported count. The shared default uses `1` for platform class `1` and an Education-specific operating-system branch. | These are physical sample counts, not slider exponents. Class `1` has not been mapped to Android, iOS or a handheld console; their current defaults remain unverified. |
| World AA | Gameplay color has an explicit MSAA resolve before post-processing. The inspected raster path provides no evidence for a full-screen FXAA pass. | Matching captures are still required for every platform and for exact terrain/entity/hand/UI coverage boundaries. |
| UI AA | UI composition has its own rendering stage. World sample count alone does not establish the sample count of every UI material. | Exact version-matched UI geometry coverage remains open; see [inventory rendering](inventory-gui-geometry.md). |
| Enhanced graphics | A gameplay color resolve precedes post-processing. | Cinnabar's Enhanced mode is a custom path; it does not claim Vibrant Visuals or ray-tracing parity. Their temporal/upscaling AA policies remain unverified. |
| Terrain filtering | The raster terrain atlas uses nearest magnification, nearest minification, linear interpolation between mip levels, and clamp addressing. Terrain atlas anisotropy is disabled. The anisotropic sampler used for the lightmap is a separate binding. | Atlas tile repetition is represented by repeated per-layer UVs in Cinnabar's texture arrays. |
| Terrain mips | `textures/terrain_texture.json` declares four mip levels. Atlas mips average source channel bytes over the original texel footprint, with truncation and no alpha-coverage rescaling. | Texture-pack mip declarations and texture size determine the available levels. |
| Terrain mip bias | No additional bias is established by the inspected terrain sampler. | Platform shader bias and deferred/upscaling bias are unverified; adding a negative bias is not a proven vanilla correction. |
| Entities | Actor color sampling comes from its material's sampler description. | A universal terrain-style sampler, mip chain or anisotropy level cannot be inferred for all entity materials. |
| Held items | The inspected ordinary item-in-hand color binding uses point filtering, including mip selection. | Specialized item materials, entity skins and their texture-loading mip policies require separate confirmation. |
| Item icons | `textures/item_texture.json` does not declare terrain's mip count. Flat icons retain their authored texels. | Exact target-version icon/model material sampling and coverage remain open; nearby-version point-sampling evidence is documented in [inventory rendering](inventory-gui-geometry.md). |
| Filtering settings | The inspected Video UI has no anisotropy or mip-bias control. | This does not prove every platform or graphics mode lacks additional controls. |
| Texel anti-aliasing | The UI includes a **Texel Anti-Aliasing** toggle gated by a capability. The inspected desktop capability disables it. Material configuration gates texel AA separately from MSAA and alpha-to-coverage. | The shared default tests platform class `2`; its platform membership, shader algorithm and current exposure are unverified. Enabling MSAA alone does not establish either texel AA or alpha-to-coverage. |

| Platform | Default AA evidence | Remaining limit |
| --- | --- | --- |
| macOS Education, inspected 26.30 desktop path | `2` samples. | Nearby-version evidence; this is not a current macOS retail Bedrock default. |
| Windows 1.26.50.26 raster | `2` samples, adjusted to the nearest supported count when unavailable. | This is the inspected preview build; retail and temporal/upscaling modes require separate confirmation. An unavailable capability list falls back to `1`. |
| Android and iOS | No platform-specific default established. | Shared platform-class branches do not identify these operating systems. |
| Xbox, PlayStation and Switch | No platform-specific default established. | Console and handheld-console defaults require separate confirmation. |

Cinnabar's desktop default of two samples follows the inspected Windows raster setting.
It does not define a mobile or console default.

Cinnabar removes FXAA, keeps nearest terrain texels within each mip, and selects MSAA from the
intersection supported by its color, depth and shadow-stencil attachments. The Video slider
shows only usable sample counts and retains the saved preference across changes in device capabilities. Its
numeric label and keyboard/pointer stops use the same sample list. Sampling changes reach the
camera before that frame publishes hand geometry.

All main-pass pipelines and private depth targets must match the selected sample count.
Post-processing reads resolved color; depth consumers must explicitly reduce multisampled
depth. Color resolve must preserve the intended gamma-space terrain blending. UI composition
remains a separate single-sample layer. MSAA must not introduce a full-screen texture filter.

Opaque, cutout, transparent, sky, world text and hand draws load the same color attachment.
Compatible encoded and sRGB views preserve each sample while retaining each material's blend
space. The last ordinary hand-rig draw resolves color directly and discards its samples; views
without that draw resolve at the end of the main pass. No pass reconstructs MSAA color from a
resolved image. Single sampling uses one final raw transfer into the post-processing image.

Shadows read the original depth samples and use an overlap stencil to multiply each covered
sample once. Hi-Z directly reduces the original depth samples to their conservative reverse-Z
minimum. Single-sample depth is created only for a post effect that requests it. Enhanced water
needs an opaque snapshot before transparency; its later post effects need the completed depth.
Enhanced keeps the hand outside grading by reusing the discarded scene attachment as a clear
transparent layer, then resolving and compositing that layer over the graded scene.

The Mac capture validates Cinnabar's Metal path, not a native macOS retail Bedrock release.
Enhanced remains hard-disabled; its attachment changes are not live-rendering acceptance.
No matching vanilla comparison or console/mobile hardware capture is implied by those results.
See `plan.md` for remaining platform, image and performance gates.
