# Shield inventory model rendering

The ordinary shield inventory icon is a model render; its `textures/entity/shield`
image is a cuboid UV sheet, not the image that should be placed directly in an inventory slot.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| GUI dispatch | Select the shield model renderer rather than the sprite blitter. |
| Shield model | Load the authored `shield` ModelPart with default geometry and `ui_shield.skinning` material. |
| Geometry loading | Find the named geometry bone and load its pivot and cubes. |
| ModelPart orientation | Use `24 - pivot.y` with box-local inverted Y and apply the supplied model unit. |

For GUI position `(x,y)` and item scale `s`, the native matrix is
`T(x + 8s, y + 10s, -10s) * S(11s) * Rx(30 degrees) * Ry(30 degrees)`;
ModelPart geometry uses `1/16` units. Matrices post-multiply, so the Y rotation acts first.
The model is not mirrored in actor-space X and does not run first-person wield animations.
For the static shield root, pivot-relative box coordinates and the root orientation combine
into `(authored.x, 24 - authored.y, authored.z)` before the GUI matrix.

| Rule | Behaviour |
| --- | --- |
| GUI translation X | `8` |
| GUI translation Y/Z | `10`, `-10` |
| GUI model scale | `11` |
| GUI rotation angle | `0.52359879` radians |
| Model unit | `0.0625` |
| ModelPart Y orientation | `24` |

The compiler resolves the shield attachable's geometry and default texture identifiers from
the pinned pack, including source-hash verification on its decoded texture. It reads the
authored cubes, dimensions, inflation, mirror and UV mappings; the shield's box dimensions
and texture colors are never copied into Rust. Other flat item icons retain their existing
route. A missing supported shield model produces no icon, never a flattened UV-sheet fallback.

The installed PlayCover vanilla `materials/ui.material` and `shaders/glsl/entity.*` corroborate
the selected GUI material: `ui_shield` inherits `ui_skinning_item`/`ui_item`, point sampling,
alpha test, blending, disabled depth test and default backface culling. `UI_ENTITY` does not
use `FANCY` face lighting. Their internal version is **1.26.51.01**, so these are corroborating
shader witnesses, not a matched 1.26.50 shader-pack claim. The bake preserves authored cube
order, culls hidden faces, samples the bound sheet, and discards sampled alpha below 0.5;
it does not reuse the provisional generic block-thumbnail lighting or depth policy.

The bounded carrier raster uses a 64-pixel canvas for the native 16-GUI-pixel frame. Synthetic
tests exercise matrix order/Y orientation, authored geometry changes, pivot cancellation,
face-UV dimensions, alpha threshold, missing-model refusal, bound texture selection,
deterministic compilation and unchanged ordinary sprites, independently of optional carriers.

Patterned/NBT-tinted shield layers and their separate banner GUI material, glint-specific
shader behavior, custom rotated/animated/inherited ModelPart trees, exact hardware-MSAA
coverage, fractional item-scale sampling and a matched vanilla frame comparison remain open.
Unsupported model-part branches are explicitly refused rather than approximated. This fixes
the ordinary shield's incorrect raw texture-sheet route; it does not close the complete shield
visual parity gate. The canonical carrier was rebuilt, and fresh offline vanilla-BDS
frames on macOS/Metal at Retina scale 2 show the ordinary model in the survival inventory,
offhand slot and HUD, rather than its raw UV sheet. Slot legibility, geometry, clipping,
layering, scale and colors were inspected, including real offhand take/place gestures.
