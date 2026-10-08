# World fire geometry

World fire uses eight sloped quads when supported, and attaches to combustible
neighbors when unsupported. The rendering target is Vanilla Bedrock 1.26.50.26.

## Vanilla rules

| Input | Geometry |
| --- | --- |
| Full top support or nonzero catch chance below | Eight supported quads; ignore side, ceiling and parity choices. |
| Unsupported, combustible west/east/north/south | Two opposite windings for each qualifying side. |
| Unsupported, combustible above | Two roof quads. |
| Unsupported, no qualifying neighbor | No quads. |

Catch chance and top support are independent effective block properties.
Destroy chance does not select flame attachments. Upper slabs and upper stairs
provide top support; double slabs support every face. Leaves reject top support.
Soul sand retains top support despite its lowered model. The current cube
fallback and incomplete flammability admission remain provisional in `plan.md`.

Up binds `fire_0`; Down binds `fire_1`. Soul fire uses the corresponding
`soul_fire_0` and `soul_fire_1` with the same topology. Each binding retains
original texels and its own `textures/flipbook_textures.json` frame order,
timing and interpolation on the shared terrain clock. Pack overrides apply.

## Geometry and UVs

Coordinates are relative to the fire cell; set `h=1.4`. Supported geometry has
four inner planes and four perimeter planes, in this vertex order.

| Plane | Binding | Vertices |
| --- | --- | --- |
| Inner A | Up | `(.2,h,1), (.7,0,1), (.7,0,0), (.2,h,0)` |
| Inner B | Up | `(.8,h,0), (.3,0,0), (.3,0,1), (.8,h,1)` |
| Inner C | Down | `(1,h,.8), (1,0,.3), (0,0,.3), (0,h,.8)` |
| Inner D | Down | `(0,h,.2), (0,0,.7), (1,0,.7), (1,h,.2)` |
| Perimeter E | Down | `(.1,h,0), (0,0,0), (0,0,1), (.1,h,1)` |
| Perimeter F | Down | `(.9,h,1), (1,0,1), (1,0,0), (.9,h,0)` |
| Perimeter G | Up | `(0,h,.9), (0,0,1), (1,0,1), (1,h,.9)` |
| Perimeter H | Up | `(1,h,.1), (1,0,0), (0,0,0), (0,h,.1)` |

Inner UVs are `(Umax,Vmin), (Umax,Vmax), (Umin,Vmax), (Umin,Vmin)`;
perimeter UVs reverse U. Bounds belong to each resolved sprite.

Unsupported sides select Up when `(x+y+z)&1` is even and Down when odd.
U reverses when `((x/2)+(y/2)+(z/2))&1` is odd; signed division truncates
toward zero. Set `b=1/16`, `t=h+b`, `A=Umax`, `B=Umin`, swapping A/B
for reversed U. Each side also emits its reverse vertex/UV order.

| Neighbor | Vertices | UVs |
| --- | --- | --- |
| West | `(.2,t,1), (0,b,1), (0,b,0), (.2,t,0)` | `(A,Vmin), (A,Vmax), (B,Vmax), (B,Vmin)` |
| East | `(.8,t,0), (1,b,0), (1,b,1), (.8,t,1)` | `(B,Vmin), (B,Vmax), (A,Vmax), (A,Vmin)` |
| North | `(0,t,.2), (0,b,0), (1,b,0), (1,t,.2)` | `(A,Vmin), (A,Vmax), (B,Vmax), (B,Vmin)` |
| South | `(1,t,.8), (1,b,1), (0,b,1), (0,t,.8)` | `(B,Vmin), (B,Vmax), (A,Vmax), (A,Vmin)` |

Roof orientation uses `(x+y+z+1)&1`, with no half-coordinate U reversal.

| Parity | Binding | Vertices |
| --- | --- | --- |
| Even | Up | `(0,.8,0), (1,1,0), (1,1,1), (0,.8,1)` |
| Even | Down | `(1,.8,1), (0,1,1), (0,1,0), (1,.8,0)` |
| Odd | Up | `(0,.8,1), (0,1,0), (1,1,0), (1,.8,1)` |
| Odd | Down | `(1,.8,0), (1,1,1), (0,1,1), (0,.8,0)` |

Roof UVs use the inner-plane order above. All vertices are white, share the
fire cell's block/sky light and omit directional shade and corner AO. The light
cache may fall back below when its first result is zero and the block permits
that fallback. Fire uses double-sided terrain cutout, world lighting/fog, depth
testing/writes and no alpha blending. Ordinary alpha cutoff is 0.5, or 0.05 with
alpha-to-coverage; the final target-version material permutation remains open.

## Smoke and acceptance limits

Ordinary fire's ambient callback produces three smoke origins for support,
combustible support, or valid placement. Each consumes X/Y/Z random floats,
placing smoke at `(X,.5+.5*Y,Z)`, with no extra chance roll. Placement excludes
campfire and soul campfire below, then accepts full top support or a combustible
neighbor. Otherwise, two smoke origins per combustible west/east/north/south/above
neighbor occupy that face's 0.1-block band. Initial velocity is zero; the runtime
`minecraft:basic_smoke_particle` controls motion, lifetime, tint and art.
Ambient crackle audio and soul-fire smoke remain separate work.

The carrier quantizes coordinates to `1/256`, so height 1.4 becomes
`358/256`. This precision difference, complete support/flammability admission,
the final material permutation and a target-version live comparison remain
open gates in `plan.md`. The installed Vanilla app is 1.26.51.01; its accepted
live comparison does not close exact-version parity. Camera and actor flames
have separate contracts: [camera fire](camera-fire.md) and
[HUD paper doll](hud-paper-doll.md).
