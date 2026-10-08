# End portal rendering

Vanilla rules:

| Element | Behavior |
| --- | --- |
| Frame | Base height 13/16; cardinal direction rotates the top texture. |
| Inserted eye | Raised cuboid from 4/16 to 12/16 horizontally, 13/16 to 1 vertically; uses `endframe_eye`. |
| Portal | Primary palette admits the surface at height 3/4 without block-entity NBT. |
| Gateway | Six outward faces span the block. |
| Surface layers | Seventeen coplanar layers encode projection depth, face normal and palette position. |
| Composition | One/one-minus-source-alpha blending, depth writes and outward backface culling. |

Pinned carrier regressions cover every frame direction and eye state, geometry,
UVs, carried textures and terrain fallback suppression. Mesh and scene tests
cover surface admission and caching; the GPU shader validates semantically.
The user accepts the rendered frames and End portal surface on macOS/Metal.

The installed native material is a near-version witness. Identical-version
pixel and color-space acceptance remains incomplete. The End dimension's sky,
dragon, crystals, healing beams and effects are separate open work in `plan.md`.
