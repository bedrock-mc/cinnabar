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

Every prediction requires loaded destination data, an air destination or verified merge,
session build-height admission, and no player or blocking-actor overlap with the resolved shape.
Unknown support shapes defer attachment placement rather than selecting a later fallback.

## Incomplete parity

Stairs and general cardinal/six-direction blocks still wait for confirmation: their direction tables
or per-type rotation rules have not been established. Doors and beds need paired-cell placement and
hinge/support validation. Rails, redstone, walls, vines, multiface blocks, signs, levers, flowers and
other substrate-sensitive plants retain confirmation until their placement and survival rules are
implemented. Candle and sea-pickle stacking, first-placement snow support, special attachment support,
and other unrecognized state keys also remain server-confirmed.

Non-air replacement stays server-confirmed. Clicked-cell selection still inherits the provisional
replacement classification used by block-use admission; complete replacement-component coverage
and the effective placement face remain an open parity gate.

The urgent mesh publication and correction tests verify local publication without acceptance.
Same-rendered-frame placement and visual parity remain open until a headless click-frame capture.
