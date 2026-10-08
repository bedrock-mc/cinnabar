# Dropped-item rendering

The runtime pack is selected by `assets/vanilla-source.json`.

## Actor origin and animation

An item actor has collision width and height 0.25 and a Y-origin offset of half
the height. Absolute movement data copies the actor's position directly, and
`AddItemActor` carries that position; the client creates the item at it and
retains it as the last received position. Client initialization keeps that
direct position and does not subtract the collision offset.

The renderer uses the interpolated riding position, which for an unmounted item
interpolates previous and current position directly, and translates by it
without subtracting the item offset. Every render dispatch builds that direct
camera-relative origin. The vanilla item origin is therefore above collision
feet, rather than an extra invented mesh lift.

Cinnabar keeps feet in the actor store, for collision boxes and brightness
sampling. The shared `protocol::ITEM_ACTOR_NETWORK_OFFSET` normalizes item
spawn and absolute/delta movement into that same space. The dropped view
restores the offset exactly once before renderer animation; first-render
camera-distance capture restores it on the current pose too. Delta packets
retain `NetworkOffset` because vanilla merges delta positions into the previous
absolute data.

The item constructor draws one uniform bob phase over a full turn.
The render Y offset uses time
`(ageTicks + partialTick) * 0.05`, sine angle `2*time + phase`, amplitude 0.1
and baseline 0.1. Ordinary block models additionally rise by 0.2. The sine
table samples `sinf(index / 10430.3779296875)`; lookup truncates `angle * 10430.3779296875` and masks to 16 bits.

On the first render only, an actor whose current origin is less than
one block from the camera captures `80 - cameraYawDegrees`. For that lifetime,
the phase is attenuated by `clamp(2*time, 0, 1)` and a 0.4-block rise eases to
zero over 0.4 seconds. The cubic ease-in is
`0.4 + (0 - 0.4) * fraction^3`, not the cube of the remaining fraction.
Spin is `max(time - 0.025, 0)` radians; the random bob phase is not added to
spin. The captured camera yaw contributes using the vanilla degree-conversion
constants and multiplication order. Walking closer later never recaptures it.

## Geometry, copies and texture selection

The item renderer draws one, two, three and four copies for stack counts
below 2, below 6, below 21 and at least 21 respectively. The renderer owns one random XYZ copy table, shared across actors; the first
copy stays centered. Later copies are translated by that table times `0.2 / groupScale` inside the spinning and
group-scaled frame, then applies actor render scale. Thus all XYZ axes spread,
spread rotates with the item, and actor/default-model scale does not stretch it.

The ordinary flat-item route uses group scale 0.3 and the shared in-hand item
renderer with the dropped-item flag, not a hand grip.
Default sprite transform applies its 1.5 scale, pixel translation
and rotations to the raster frame. Combined with sprite extrusion, this maps a raster point `[column, depth, row]` to local
`[0.5 - column/maxSide, 1 - row/maxSide, -depth/maxSide]`; net group scale is
0.45. The texture's longer side controls scale, not a separate width/height
stretch. Pixel-depth mode gives a 1/16 local slab. Alpha values zero and one
produce no extrusion edges, and the rear face samples the same pixel rather
than a separately flipped image. Static item-frame/campfire placements retain
their existing mesh path and do not inherit this dropped-item transform.

Ordinary block drawing uses the dropped display mode. Default
block display has pixel translation `(0, 3, 0)` and scale 0.25;
the mode correction translates down by 3/16, canceling
that default translation. Centered unit cubes therefore use scale 0.25.
Known cubes reuse the compiled carried-face sheet when its manifest matches
the world catalog, rather than projecting a GUI cube thumbnail or borrowing
biome-tinted terrain faces. Grass soil and side colors consequently keep the
authored carried appearance. See [carried-block-textures.md](carried-block-textures.md).

The legacy tessellation route queries the item's animation frame before
its icon. Loaded crossbow sprites share the same animation-frame icon selector as HUD and inventory icons, using canonical charged
projectile NBT rather than damage metadata. Resolved identifier and variant
are part of the dropped model cache key. See [crossbow-use.md](crossbow-use.md).

## Verification and remaining boundaries

Synthetic regressions cover copy-count thresholds, shared isotropic offsets,
stable lifetime phase, actor render scale, half-tick spin, empty/invalid actors,
consistent spawn/absolute/partial-move origins, feet-anchored boxes and
brightness, vanilla sine lookup, first-render camera capture/easing, rotated
unscaled spread, sprite floor origin, non-square sprites, rear UVs, alpha-edge
admission, carried-face colors and shared loaded-icon selection.
Fresh offline vanilla-BDS frames on macOS/Metal at Retina scale 2 show an
ordinary Dirt cube (`2026-10-02_00.28.21.png`) and a dropped 21-Diamond stack
(`2026-10-02_00.24.26_1.png`). The cube uses all six carried faces; the diamond
uses extruded pixel geometry and overlapping stack copies rather than a flat
inventory thumbnail. Floor placement, bob/spin, legibility, geometry, clipping,
depth/layering, scale and colors were inspected on a locally lit test pad.
Walking over each removes the ground actor. These are live functional/device
frame checks, not a version-matched vanilla gallery or pickup-trajectory parity.
Inventory pickup authority is separately covered by the normal transaction
receive path; seeing an actor disappear alone does not establish its count.

Incomplete: shield, banner, decorated-pot, exceptional non-cube blocks and authored custom dropped display
transforms require their vanilla model routes, not a flat GUI thumbnail. This
change corrects ordinary sprites and cubes but does not close those gates.
Multi-layer/enchantment material behavior, vanilla actor lighting/shader parity
and non-default texture-depth data remain incomplete. The existing dropped
shader's directional shade and alpha threshold were not replaced by this
geometry correction. Visual RNG ownership and distributions match vanilla,
but the local generator is ours, not the vanilla generator; platform sine
rounding is not claimed bit-identical.

Pickup feedback owns a copied item presentation independently of server removal.
It flies for three ticks with squared progress toward the collector's native origin
minus 0.6 on Y and shrinks its meshes by the remaining progress. Bob, block lift
and stack-copy spread retain their ordinary translations during the flight.
The pickup event commits one
pack-routed `pop` sound even if removal arrives in the same batch. A version-matched
rendered pickup comparison remains incomplete.
