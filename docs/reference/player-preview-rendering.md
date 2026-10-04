# Player model rendering inside JSON-UI

The inventory model must be rasterized from geometry at the active UI framebuffer's physical
resolution. Enlarging a pre-rendered 96×112 image loses geometric coverage and skin texels;
point sampling cannot restore them. Minecraft's intentionally coarse skin artwork is separate
from this extra silhouette/downsampling artifact.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Inventory live model | Render actor geometry at the control’s physical framebuffer resolution using the live model scale and pointer-driven pose below. |
| Cursor coordinates | Measure pointer offset in GUI coordinates. |
| Paper doll | Use the authored starting rotation and camera tilt with the paper-doll scale below. |
| UI shading | Force white color/light vectors and clear world overlay/fog values. |
| Actor dispatch | Set UI-rendering state and dispatch the actual actor geometry. |

Current live rendering takes `min(control_width, control_height)` as the model scale, translates
to the control's center, and applies the actor's eye offset. The pointer offset is measured in
GUI coordinates. Its body yaw is `atan(dx / 40) * 20` degrees, head yaw
`atan(dx / 40) * 40`, and head/model pitch `atan(dy / 40) * -20`.
The paper doll uses `min(width / 20, height / 39)` per model pixel, with the renderer's
`starting_rotation` and `camera_tilt_degrees` properties.

The live path builds a screen-context alpha override, creates an actor render context, sets
UI-rendering state, and dispatches the actual actor geometry. The ordinary paper-doll path
similarly uses its screen context; offscreen capture is a distinct optional request, not a
mandatory small fixed-resolution inventory image.

When actor render data selects UI rendering, shading forces white color/light vectors and clears
world overlay/fog values, instead of evaluating the world-light branch. This
does not justify the old software raster's invented `0.62 + max(dot(N, light), 0) * 0.38` shader.

## Texture and shader corroboration

The install-fetched player pack selects `entity_alphatest` and model scale `0.9375`.
Installed PlayCover material and GLSL files are a near-patch corroborating reference
(internal version `1.26.51.01`), not a substitute for a version-matched material comparison:

- `data/resource_packs/vanilla/materials/entity.material`: entity geometry uses point sampling,
  the `entity_alphatest` family enables alpha test and disables culling, and MSAA is supported.
- `data/shaders/glsl/entity.fragment`: normal alpha-test textures discard alpha below `0.5`.
- `data/shaders/glsl/entity.vertex`: ordinary directional intensity is `1.0`. The distinct
  `FANCY` branch uses `AMBIENT=0.45`, `XFAC=-0.1`, `ZFAC=0.1` and the transformed normal.
- `data/resource_packs/vanilla/materials/fancy.json` adds `FANCY` to the complete
  `entity.material` group; `sad.json` loads that group without it. `common.json` loads the
  separate UI material group, so `ui_item`'s unlit behavior is not evidence for unlit players.

Both native directional branches are implemented. UI setup gives
`TILE_LIGHT_COLOR=(1,1,1,1)`, including light direction `+1`. For normalized transformed
normal `(x,y,z)`, fancy intensity is `(1+y)/2 * (1-0.45) - 0.1*x*x + 0.1*z*z + 0.45`.
The value remains an interpolated float, separate from byte-authored dye/tint; it is never
rounded into an sRGB color byte. The ordinary branch gives `1.0`. The current client selects
fancy as the current desktop default. Complete graphics-mode/backend program selection,
texture color-space equivalence and hardware MSAA coverage still require a matched frame gate.

## Cinnabar implementation

The HUD keeps its controller clocks and Molang variables separately from the world and
first-person hand evaluation. The native actor selects the UI animation component while
UI rendering is active, and the HUD sets
`variable.is_first_person` to zero before drawing that same actor. The installed pack's `animation_controllers/player.animation_controllers.json` root
starts in `first_person` and enters `third_person` when `!variable.is_first_person`.
Retaining its state lets the keyframes in `animations/player.animation.json`,
`animation.player.crawl`, advance after the transition.

Live HUD geometry resolves from the catalog that owns the current rig, including a
server-pack catalog's separate binding index space. Its cache includes the catalog's
source digest so another session's rig at the same index cannot reuse old vertices.
This follows the native HUD's lookup of the actor's renderer and ordinary actor draw. The offline regression uses original
one-cube server rigs with different dimensions and needs no installed carrier.

`player_preview/geometry.rs` emits normalized JSON-UI triangle geometry from the shared biped
base/overlay vertices and current native view transforms. The old virtual preview dimensions
are only the retained node's coordinate basis: there is no small intermediate framebuffer.
Original skin/armor texture regions supply the UV edges, including HD skins; the GPU samples
them at the destination's actual pixel coverage. `validated_ui_skin` preserves each square
skin's original allocation/dimensions and uses the shared Bedrock limb expansion only for
legacy half-height skins. It does not resample to the world-renderer skin array's fixed side.
Atlas UV coordinates remain floating point, preserving original edges and native extrusion
side texel centers without rounding or clamping. Non-finite/out-of-range mappings are rejected.
Armor keeps its own texture dimensions/dye.
One isolated depth interval is shared by skin, armor and held carriers, preserving part
occlusion without inheriting terrain depth or quantizing the silhouette to preview pixels.

## Held geometry, rather than GUI thumbnails

Visible held previews use original model-space geometry and its texture source, not an
inventory-projected icon pasted onto an arm. Opaque carried cube blocks use the shared
`textured_cube_vertices` face/UV contract; legacy sprite items use `held_sprite_vertices`,
including its one-texel extrusion and exact side texel centers. Both hands select independent
stack identity, metadata and charged-projectile state. The factory resolves `rightItem` and
`leftItem` origins from the actual player rig binding and named geometry pivots; it does not
invent a hardcoded hand offset when a source bone is absent.

For ordinary flat sprites its grip is
`T(.3125,.1875,-.1875)*S(.375)*Rz(60)*Rx(-90)*Rz(20)`; hand-equipped sprites use
`Ry(180)*T(.1,.265,0)*S(.625)*Rx(80)*Ry(45)`. These precede the common item default
transform, and are shared with Cinnabar's world equipment renderer instead of duplicated.
Current offhand first adds `T(-.125,0,0)` in the reference hand-bone frame;
its hand-equipped translation is `T(0,.265,0)`, not the main-hand `.1` X translation.
Ordinary flat sprites otherwise retain the same grip. There is no mirror-main-hand shortcut.

Legacy cube display supplies
`T(0,.1875,-.3125)*Rx(200)*Ry(225)*S(.375)`. Offhand ordinary cube rendering reaches that
same helper after its independent hand-frame offset. `_rebuildItem` uses mesh offset `(-.5,-.5,-.5)`, centering the cube basis. Native special-shape/custom presentation branches remain distinct.

Single-bone authored Shield/Trident previews retain actual attachable geometry, bind pivot,
texture and literal pack channels. Bound-root channels are composed with the owning item
bone and the authored bind pivot is removed exactly once. They receive neither a legacy
sprite grip nor a GUI-item projection. See [held attachables](held-attachables.md) for the
native parent/binding contract. Item normals follow the same bone, arm and view transforms
before the native float FANCY formula; they are not guessed from projected icon triangles.
Expression-bound roots retain
the authored default root origin with Y pivot minus the shared ModelPart height.
`PreviewHeldPlacement::authored` applies that origin adjustment once, retaining the original
mesh bind pivot; it does not rotate an invented screen offset into the model. The player
pose VM and literal preview share `MODEL_PART_ORIGIN_Y` with the existing GUI ModelPart basis.
The first live preview run caught an offhand Shield floating above the head because this
bound-root origin was missing. That rejected capture is not a passing parity gate. The
installed-carrier regression now exercises the actual Shield geometry, both authored wield
poses and actual player item-bone pivots, checking bounds at the hand instead of above the head.
The native mirrored actor-rig frame converts once into the standard preview biped's front
frame before its arm/owner projection, keeping the item seated as the live model turns.
`DataDrivenRenderer::render`
uses Y rotation `wrap(180-body_yaw)` and then negates the first two matrix columns.
The live UI outer fixed rotation is Z rotation by pi: its contiguous axis
is `(0,0,1)`, not `(0,1,0)`. The neutral outer/actor X/Y sign rotations cancel, leaving
the actor's fixed facing half-turn; that establishes the held producer's front-frame
conversion. It is not a GUI thumbnail mirror or a second authored binding transform.

The software raster remains a compatibility/test and empty-hand fallback. It is not the
quality path for the inventory's live player or paper-doll model.

GPU pose/view angles and native idle bob are continuous, not CPU-cache quarter/half-degree
steps. `animation.player.bob` uses `cos(life_time*103.2)*2.865+2.865` in degrees. The `PaperDollRenderer` constructor sets `variable.is_paperdoll=1`; the install-fetched
player controller's paperdoll state omits bob, holding and sneak animations, so that renderer
does not inherit the live model's idle arm/crouch/holding modifiers.

Incomplete parity gates: custom/slim/non-square skin geometry, capes/persona parts,
renderer-specific full animation evaluation, multi-bone/dynamic held attachable controllers
(including authored Bow/Crossbow poses), special-shape/custom block presentations,
native per-item hand-equipped bit flags instead of the existing legacy identifier predicate,
rebuilding GUI model sources on live server/global resource-pack changes,
all material/glint variants, complete graphics-mode program selection,
native hardware sample coverage, and a version-matched controlled native frame comparison.
Moving geometry to the physical-resolution UI draw path does not by itself close those gates.

## Live follow-up acceptance

The canonical `target/debug/bedrock-client` was rebuilt and launched on macOS 26.3,
Apple M3 Pro / Metal, at 1280×720 logical / 2560×1440 physical (Retina 2).
The existing offline loopback official BDS test session was retained; only the Rust
client restarted, with ordinary server-given fixture stacks and real inventory moves.

Fresh full-resolution F2 frames beneath the ignored `.local/screenshots` directory:

- `2026-10-02_03.22.01.png`: the selected Dirt preview has real cube faces, not a
  GUI thumbnail painted on the arm. Original skin/art texels remain point sampled
  while geometry is rasterized at display resolution.
- `2026-10-02_03.22.23.png` and `03.22.44.png`: Dirt plus offhand Shield remain
  seated at their hands while the pointer changes the live head/body pose. The
  rejected above-head Shield is absent; handle/arm depth and model-control clipping
  were inspected in the corrected build.
- `2026-10-02_03.23.12.png` and `03.23.38.png`: the extruded Diamond Sword follows
  its own native tool grip, with the offhand Shield still seated independently.
- `2026-10-02_03.24.17.png`: moving the Shield back into the main hand selects its
  separate authored wield pose, with no above-head root or duplicate offhand model.

Inventory close/reopen, slot selection, moving the Shield between hands, preview
turning, texture colors, legibility, geometry, clipping and depth/layering passed
this target-platform rendered-frame check. Computer Use refreshed discovery and
captured the window; after its focus/input failure, the explicitly recorded fallback
used PID/frontmost-guarded native events for input and the client's F2 framebuffer
capture for unresampled Retina images. No screenshots or runtime assets enter git.
This closes the reported flat-held-thumbnail/floating-bound-root regressions on
this setup, not the incomplete native frame/material/animation gates above.

### Upstream-integrated rerun

After integrating `dev` through `0979ff223974b9752a6fe9535fcb787d90fe8ade`,
the canonical client and core were rebuilt. This rerun used the same macOS/Metal Retina-2 configuration and the
existing world reopened through the local-world manager, not a regenerated world.
The manifest-pinned official BDS ran on `127.0.0.1:56399`; core logs confirmed
offline authentication and BDS reported an empty XUID.

Fresh framebuffer witnesses:

- `2026-10-02_04.09.09.png`: Dirt's real held cube and original skin texels.
- `2026-10-02_04.09.45.png` and `04.12.03.png`: Dirt/offhand Shield with
  opposite pointer-driven poses, seated roots, correct occlusion and model clipping.
- `2026-10-02_04.10.41.png`: Diamond Sword's independent tool grip and offhand
  Shield, with the two-line enchanted-item tooltip above the inventory.
- `2026-10-02_04.11.13.png`: Shield's separate main-hand wield pose; the offhand
  control and model clear after moving the stack back.

The upstream stateful binding path exposed another regression during integration:
an empty cell omitted `#item_renderer_data`, retaining an old compact icon-table
index that could now point at another item. Inventory and HUD providers now answer
with explicit null for an empty/unrenderable cell; the custom renderer distinguishes
that answer from an absent legacy binding. Null does not fall back to item-aux data.
The integrated frames show the third hotbar cell blank in both views after moving
the Shield out, correctly refilled when moved back, and blank again after repeated
close/reopen and moving it out. Stateful occupied/empty/refilled tests cover both
providers, plus malformed indices and explicit-null precedence. This is a retained
UI transport correction, not a change to server inventory transaction semantics.

Legibility, geometry, clipping, depth/layering, scale, colors and real hover/slot/
focus behavior passed this integrated target-platform rerun. It does not close the
incomplete native comparison, material or dynamic attachable gates above.

Final integrated verification passed: the carrier rebuild, canonical Rust/Go
builds, focused icon-pipeline regression, `cargo test --workspace --locked`
(four test threads), `cargo fmt --all --check`, strict workspace/all-target Clippy,
the repository architecture check, `go test ./...` and `go vet ./...`. The first
workspace run rejected the old icon-pipeline assertion because it counted an
explicit null as a drawable item; the corrected test checks numeric drawable
bindings and null clear answers independently. The complete rerun passed.
After the final normal launch rebuild and relaunch, `2026-10-02_04.24.12.png` again shows Dirt/offhand Shield and the Grass
Block tooltip; `04.24.25.png` shows a reopened inventory with the emptied cell
blank in both views and no tooltip when hovering that empty cell.
