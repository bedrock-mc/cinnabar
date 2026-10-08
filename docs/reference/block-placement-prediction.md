# Local block placement

Gameplay resolves a placed state before transport admission, then writes it locally after the
transaction is admitted. Authoritative block updates replace it through the existing correction path.
State lookup uses an index built when the collision registry loads.

## Vanilla rules

| Family | Local rule |
| --- | --- |
| Stateless blocks | Preserve the held state; refuse unresolved survival rules. |
| Pillars | The clicked face chooses the X, Y or Z axis. |
| Slabs | Bottom/top faces choose top/bottom; a side hit above the midpoint chooses top. |
| Matching slabs | Merge the exposed clicked half, or a matching slab in the destination, into the default double state. |
| Trapdoors | Quantize player yaw with the trapdoor direction encoding; select the half from face and hit height. |
| Hoppers | Face toward the clicked support, with vertical clicks pointing down. |
| Torches | Try the clicked support, then supported walls in their defined order, then the floor. |
| Buttons | Use the clicked face and reset the pressed bit; require opposite-face support. |
| Lanterns | Prefer the floor, except a ceiling click with valid ceiling support; otherwise hang from the ceiling. |
| Colored carpets | Require a non-air block below. |
| Existing snow layers | Increment height below the maximum while preserving other state; check actors against the whole cell. |
| Fences and panes | Resolve neighboring connections before collision validation; rendering derives connections from the same neighborhood. |
| Stairs | Use the stair yaw encoding and strict side-hit midpoint. Resolve the placed corner and changed horizontal stair neighbors together. |
| Loom and glazed terracotta | Use their individual four-way direction tables. |
| Dispensers and pistons | Nearby placements above/below the player use strict body bounds; other placements use the yaw table and the type's rotation offset. Reset dispenser triggering. |
| Doors | Resolve lower and upper cells together, with the door rotation and upper-half hinge. Require two air cells and floor support. Hinge neighbors are limited to known air, stone and doors. |
| Levers | Wall clicks choose the attachment direction; floor/ceiling clicks choose the axis by yaw. Preserve the open bit. |
| Ordinary signs | Top clicks select the matching standing sign and one of 16 rotations. Side clicks select the matching wall sign and clicked face. |
| Unlit redstone torches | Use the shared torch placement and ordered support fallback. |
| First dry vines | Horizontal clicks set one attachment bit. Predict only stone-supported placement into air. |
| First dry glow lichen and sculk vein | Try the opposite clicked face, then the ordered support fallbacks; reset the held mask to the selected bit. |
| First snow layers | Reset height and cover state. Predict only known stone, leaf or full snow-layer support. |
| Candles | First placement resets count and light state. Same-color top/side clicks add one candle below the maximum, preserving light state; a full top stack refuses placement. |
| Dry sea pickles | First placement and top/side stacking reset the dry bit and use counts from one to four. Underwater placement stays server-confirmed. |

Every prediction requires loaded destination data, an air destination or verified merge,
session build-height admission, and no player or blocking-actor overlap with the resolved shape.
Unknown support shapes defer attachment placement rather than selecting a later fallback.

## Incomplete parity

Directional types outside the explicit table, including droppers, observers, barrels and furnaces,
remain server-confirmed until their per-type callbacks and rotation offsets are established.
Beds need verified support traits and local block-entity visual publication. Walls need their
above-block collision rules; rails and redstone dust need complete connection and neighbor rules.
Flowers and other substrate-sensitive plants need complete substrate membership.

Existing vine/multiface masks, wet attachments, special support shapes, unknown door-hinge
neighbors, snow covering plants, hanging signs, and unrecognized state keys also defer.

Non-air replacement stays server-confirmed. Clicked-cell selection still inherits the provisional
replacement classification used by block-use admission; complete replacement-component coverage
and the effective placement face remain an open parity gate.

## Publication and verification

All cells of a prediction are prepared before any are committed, including across sub-chunks.
A later authoritative update replaces each predicted cell. Lighting and mesh work start at local
commit, and a bounded late worker poll can publish completed work without admitting newer server
events. The late poll never blocks the frame waiting for a worker.

Developer-control exposes the latest placement's commit, render-queue handoff and main-thread
upload acknowledgement frames. A handoff or acknowledgement is not a presentation receipt.
Table and immediate-prediction tests cover both palette encodings; pipeline tests cover
cross-sub-chunk pair rollback, urgent meshes and ordered corrections.

A headless 120-fps fixed-clock stair recording with the server paused first shows the stair
three frames after local commit; staging is also +3 and observed upload acknowledgement is +5.
Both door halves also render with the server paused in a separate capture. These are limited fixtures,
not full hardware budget measurements. Same-rendered-frame visibility and complete visual
parity remain open because lighting and meshing complete asynchronously.
