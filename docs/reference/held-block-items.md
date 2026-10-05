# Held block items

The target selected by `assets/bedrock-target.json` chooses block tessellation from
the item's rendering block shape. Terrain occlusion, flammability, tint and alpha
do not turn a cube into an extruded inventory icon.

Full cubes use six pack faces, preferring authored carried textures and otherwise
the block's ordinary texture variant. The carrier's transparent cube templates
retain that same geometry. Carried overlay alpha resolves tint masks; ordinary
cutout and blended face alpha is preserved.

Material selection remains independent: opaque and surviving cutout samples write
opaque color, while blended samples composite over the scene. Main and offhand
use the same cached geometry and material classification. Cutout samples below
half alpha discard and both sides render; opaque and blended cubes cull backfaces.
Non-cube shape parity and animated or high-resolution carried sheets remain
separate open work.
