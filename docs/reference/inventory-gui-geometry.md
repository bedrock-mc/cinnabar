# Inventory geometry at display resolution

The previous ordinary cube icon path baked a complete projected block into 16×16 pixels;
carried/model cubes used 32×32 and shields 64×64. Enlarging these finished thumbnails with
point filtering discarded the geometric coverage and most source texels before the visible
UI render. Point sampling the original item/skin art is not itself the defect: vanilla uses
point sampling too. The new model route submits geometry through the retained JSON-UI draw
list so its triangle coverage is evaluated at the current physical framebuffer resolution.
Flat item sprites retain their original authored pixels and point-sampling route.

### Vanilla rules

| Rule | Behaviour |
| --- | --- |
| GUI dispatch | sprite versus block versus special item renderer |
| Shared GUI routes | Shared GUI item routes, including ordinary block type geometry |
| Block GUI transform | Ordinary block GUI translation and per-axis scale |
| GUI tessellation | Block GUI tessellation, face colors and immediate mesh submission |
| GUI rotation | Tessellator default GUI rotation matrix |
| Vertex transform | Vertex transform before scale/translation |
| Mesh submission | Ends tessellation and submits the material/texture mesh |
| Shield GUI transform | Shield GUI transform and ModelPart draw |

For an ordinary full cube with GUI scale `s`, the default block transform is
`T(x+s, y+12.4799995s, z) * S(10s) * Rx(210 degrees) * Ry(45 degrees)`.
The origin calculation uses the block item rendering-shape factor; for an
ordinary cube its factor is one. The GUI matrix constructs the X rotation followed by the Y
rotation, so Y acts first on authored points. The native rotation angles have bits
`0x406a927f` and `0x3f490fdb`; the scale/vertical offset have bits `0x41200000` and
`0x4147ae14`. The displayed icon frame is 16 design pixels, not 16 physical pixels.

The ordinary cube GUI path emits only Up, South and West, in that order. Their native
vertex brightness values are `1`, `0.5` and `0.730000019` for non-fullbright blocks.
The GUI tessellator converts each channel by truncating `255 * brightness`, producing bytes
`255`, `127` and `186`. This is vertex modulation of original face textures, not rounded
pre-shading of a baked icon. Runtime sources retain carried grass face colors when present;
ordinary faces come from the same material texture layers as the block registry.

The shield route uses its own pack geometry and default bound texture, with
`T(x+8s,y+10s,-10s) * S(11s) * Rx(30 degrees) * Ry(30 degrees)`, model units `1/16`.
Static ModelPart pivot-relative coordinates combine into `(x,24-y,z)` before the GUI matrix.
Authored cube/face order is preserved; backfaces are excluded, face colors remain white,
and sampled alpha below `0.5` is discarded before any UI opacity modulation.
See [shield inventory rules](shield-inventory-icon.md) for the ModelPart witnesses.

The installed PlayCover material/shader source is **1.26.51.01**, a near-patch corroborating
witness, not a matching shader claim. `ui_item` and `ui_shield` inherit point sampling,
alpha blending and disabled depth testing; `ui_item` uses its vertex tint-mask branch,
while `ui_shield` uses sampled half-alpha testing and no fancy face lighting. Both model
builders therefore disable depth test/write and preserve native authored draw order.

The shared UI renderer keeps upstream's gamma-space, single-sample compositing layer,
including ordered invert overlays and accessibility glint factors. JSON-UI model controls
retain floating-point UV/light attributes and use a private depth surface matching that
layer's actual physical extent/sample count, not the terrain depth/MSAA target. A control
clears its depth once; later material passes retain it without clearing earlier UI colors.

This does not close the complete visual parity gate. Version-matched hardware coverage/MSAA,
exact native model-material texture/color transfer-function formats and a controlled vanilla frame comparison remain
unverified. Fullbright cube overrides, special block GUI shape factors/tessellation, patterned
shield NBT layers, native glint and custom rotated/inherited/animated shield ModelParts
remain incomplete. Mesh UV transport preserves authored floating-point texel coordinates,
including extrusion side texel centers; ordinary sprites/glyphs retain integer-edge sources. Unsupported
model branches keep the previously documented fallback route; they are not labeled exact.
