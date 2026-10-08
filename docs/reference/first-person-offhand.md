# First-person offhand placement

This records vanilla behavior; it does not claim that every route below is
implemented or visually accepted. We implement these contracts in our own code.

## Separate render path

Offhand rendering selects the renderer's cached offhand stack, builds its render
key using animation frame `-1`, pushes the camera matrix, and draws the cached
item. It does not re-enter the ordinary main-hand item transform after applying
its camera pose.
There is no generic mirror-main-hand shortcut.

Maps, modern attachables, blocks, authored display transforms and legacy items have
distinct routes. The default legacy route is admitted by the item fallback or the
non-old-tessellation block display flag. Outside that route, the renderer selects the
authored world-matrix transformation instead.

## Default legacy sprite matrices

The following are post-multiplied onto the incoming camera matrix, in written order.
Angles below are degrees. These are vanilla tessellator-frame matrices, not an instruction
to apply them directly to an unrelated centered mesh basis.

For items that are not hand-equipped:

```text
Rx(90)
* T(-1.25, -1.125, 0)
* Rz(-80)
* Ry(-20)
* T(-0.3125, 0.25, -0.03125)
* S(1/16)
* S(16/max(icon_width, icon_height))
```

For hand-equipped items, unless the legacy Shield-blocking special case wins:

```text
T(-0.6875, -0.125, -1.53125)
* Ry(-10)
* Rx(70)
* Rz(80)
* S(1/16)
```

The hand-equipped predicate reads the item’s hand-equipped flag (bit 1); it is
not inferred from the identifier.

### Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Flat sprite X rotation | `pi/2` |
| Flat sprite negative X translation | `1.25` |
| Flat sprite Y translation | `-1.125` |
| Flat sprite Z/Y rotations | `-80`, `-20` degrees |
| Final flat sprite translation | `0.3125`, `0.25`, `0.03125` |
| Hand-equipped translation | `0.6875`, `-0.125`, `1.53125` |
| Hand-equipped rotations | `-10`, `70`, `80` degrees |
| Pixel-to-model scale | `0.0625` |

The table shows equivalent angles; the vanilla rotation constants use radians. The hand-equipped depth is `1.53125`,
not the ordinary block depth `0.72`.

## Geometry normalization

Rebuilding the cached item stores `16/max(width,height)`. The flat offhand branch
reads that same field; there are not two independent scaling fields.

The flat branch therefore normalizes the vanilla pixel geometry to one model unit on
its longest side. Cinnabar's `held_sprite_vertices` already normalizes the longest
side and uses the held slab frame (X non-positive, Y non-negative, depth toward -Z).
Its X mirror and origin shift must not be repeated as if it were the centered
`extruded_sprite_vertices` mesh. Modern texture meshes retain that separate centered API.
Sprite extrusion emits positive column
X, depth Y and row Z in texel coordinates. For a UV-labelled held-slab point the exact
normalized vanilla point is `(-held.x, -held.z, height/max - held.y)`. Thus the basis
conversion is `T(0,0,height/max) * Ry(180) * Rx(90)`. It is a proper rotation, not a
UV reflection. The flat branch already has longest-side normalization; the
hand-equipped branch additionally scales by `max/16`. Regression tests compare
front/back texel corners at square, rectangular and higher-resolution dimensions.

## Blocks and Shield legacy alternative

The block default branch starts at `T(-0.56,-0.52,-0.72)`, then selects the block's
display transform for presentation type `2`. The default presentation has zero translation/pivots, Y rotation `-135`
degrees and scale `0.4`. Type 2 negates Y/Z, yielding
`T(-0.56,-0.52,-0.72) * Ry(135) * S(0.4)`. The translation constants are `0.56`, `-0.52` and `0.72`. Do not substitute the main-hand presentation type or sprite scale.

The legacy Shield-blocking route uses X/Z/Y rotations `2.5`, `177.5`, `-2` degrees,
then its offhand-height-dependent translation and scale `1.125`.
Modern Shield attachables instead use their authored animation and owner binding; see
[held attachables](held-attachables.md) and [blocking state](shield-blocking.md).

## Offhand render context

First-person rendering writes `context.player_offhand_arm_height` separately from `variable.player_arm_height`.
The offhand value is
`previous_offhand_height + (current_offhand_height-previous_offhand_height)*frame_alpha`;
main-hand heights interpolate their own previous/current pair the same way.
The ordinary flat/hand-equipped offhand branches above do not apply the main-hand
attack-time swing stack. They also do not inherit the avatar's main-hand item bone.
There is no generic equip-height dip on these legacy branches: the only reads of
offhand height in offhand rendering are in its legacy Shield special case. The
outer first-person function calls offhand rendering before entering the main-hand
camera/equip stack, with a matrix push and the screen
aspect-layout adjustment but no offhand-height translation. Modern attachables use
the independently interpolated context through their authored first-person animations.

The renderer tick snapshots both hands independently, advances each toward
zero for a lowering transition or one otherwise, clamps each change to `[-0.4,0.4]`,
and replaces the cached stack at height `<=0.1` (or on an instant-update transition).
Both current and previous offhand heights initialize to zero. Cinnabar now retains this independent clock across pose
resets and supplies its interpolated value to the shared attachable VM.

Incomplete gates: the complete map/legacy Shield route, authored display transforms,
and stack-specific vanilla instant-update/equivalence predicates (the current retained
equipment feed supplies identifiers, not the full vanilla cached-stack comparison).
The October 1 offline vanilla-BDS run on macOS/Metal at Retina scale 2 renders
the offhand Shield alongside main-hand blocks and the Crossbow, with independent
texture bindings and poses. The open survival inventory shows its offhand icon;
real take/place/restore gestures leave both that icon and the hand visible.
Geometry, clipping, layering, scale, colors and input ownership were inspected
in fresh rendered frames. This is functional/device rendering evidence, not a
matched vanilla gallery or acceptance of the incomplete routes above.
