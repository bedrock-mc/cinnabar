# Liquid-current admission and impulse

This describes the vanilla 1.26.50.26 client. Cinnabar's implementation is
independently written.

## Vanilla rules

Each tick vanilla writes the flow policy, then fetches liquid blocks, then runs
lava, water and head sensing. Only afterward do player triggers and pose
updates run. Consequently current acts on retained velocity before jumping,
swim steering, relative acceleration and collision resolution. It samples the
preceding pose box at the current feet, without sweeping by velocity.

The ordinary player's water probe shrinks X/Z by `0.001` and Y by `0.401`;
lava uses `0.1` and `0.4`. Inverted axes clamp to the original center. The
material gather traverses Y, then Z, then X, with a floored lower bound and
exclusive `floor(f32(upper + 1))` range end. It gathers resolved-liquid cells:
the extra non-air block takes precedence, with primary fallback when extra is
air. When both probes contact their liquid, the fetch selects lava for current
force.

The local flow-policy writer admits ordinary current when the prior flying
ability is clear. The fly trigger runs later in the tick, after swimming intent.
The input therefore retains the preceding flow allowance for live ticks and
replay, independently of the newly selected flight mode.

Flow additionally requires a collected raw `liquid_depth` greater than zero.
When all collected depths are zero, the fetch checks only the collection's
outer horizontal faces in Z-, X+, Z+, X- order. A neighboring resolved block
with matching material and nonzero raw depth admits current. The boundary bits
for those faces are `0x04`, `0x20`, `0x08` and `0x10`, respectively. Falling
depths still admit current; collapsing them to an effective depth zero too early
would incorrectly disable the force.

Once admitted, every collected cell contributes its per-cell flow vector,
including source-depth-zero cells. Each cell's vector is already normalized.
The fetch sums these vectors in Y/Z/X cell order, normalizes the aggregate
again, multiplies by the selected material's impulse, and adds it to the
retained velocity. The force does not grow with the number of touching cells
and is not an average of separately scaled impulses.

Both normalization steps compute float squared length grouped as
`(X² + Y²) + Z²`, then float square root, accepting lengths at least `0.0001`.

| Float | Meaning |
| ---: | --- |
| `0.0000999999975` | Normalization threshold |
| `0.0140000004` | Water impulse |
| `0.00350000011` | Lava impulse |
| `1` | Exclusive gather end addition |

## Cinnabar implementation

`world/current.rs` implements this aggregate and admission. Its production
query retains the identities of body, boundary and per-cell vector reads; the
immutable collision snapshot forwards the same query for replay. Material reads
are memoized and bounded. A proved dry or still-water result returns an explicit
zero impulse. Missing raw liquid depth preserves material contact but supplies
no current authority when that depth is required by selected body cells,
boundary admission or a per-cell vector. Rendered liquid height does not replace
the raw state. A legacy world or a per-cell vector whose obstruction facts are
unavailable also returns no current authority, preserving the incomplete
boundary instead of inventing a vector from collision geometry. Mounted
ownership and non-player/item-actor liquid probes remain outside this player
implementation.

The integrated client passed a locked local build. Tests and the affected
verification suite were skipped, following the user's explicit instruction.
The user accepted flowing-water movement in the rebuilt macOS/Metal client
connected to the local BDS on 19132. This manual check covers the reported
current defect; it does not close the unsupported boundaries below.

## Per-cell direction and obstruction facts

`world/flow.rs` implements the vanilla per-cell liquid flow direction.
Production registrations retain BREG `ModelStateField::LiquidDepth` without
collapsing values 8–15. The helper uses raw depth for the falling branch and
effective depth zero for those values when comparing neighboring levels.

Planar facings are visited as `2, 5, 3, 4` (Z-, X+, Z+, X-). Face masks are
`1 << facing`, and the opposite facing swaps each adjacent facing pair. For a
matching neighboring liquid, both primary blocks must admit the corresponding
face. The contribution is `(neighbor depth - current depth)` times that
direction. A rejected face follows the same fallback as an unmatched liquid:
when the primary neighbor's material does not block motion, a matching liquid
beneath it contributes `(below-neighbor depth - current depth + 8)` times that
direction.

The getters are intentionally distinct. Neighboring and below-neighbor liquid
depth use the extra-then-primary resolved block; the neighbor's blocks-motion
material flag, directional face checks and the falling-wall checks all read the
primary block.

For raw depths at least eight, a solid primary neighbor or its block one cell
above causes the horizontal result to be normalized first, adds Y = `-6`, and
normalizes again. Each arithmetic step remains float; widening happens after
the normalized vector is complete.

Each material carries a blocks-motion flag and an is-solid flag. Types 0, 5 and
6 have both clear; types 1, 13 and 23 have both set. The bindings below come
from each block's material type, not PREG passability, render opacity,
collision boxes, or BREG face coverage.

| Block | Material type |
| --- | ---: |
| Air | 0 |
| Water / flowing water (raw BREG LiquidDepth) | 5 |
| Lava / flowing lava (raw BREG LiquidDepth) | 6 |
| Dirt | 1 |
| Grass | 1 |
| Stone | 23 |
| Sand | 23 |
| Gravel | 23 |
| Ice / packed ice | 13 / 23 |

For every listed block and for liquids, the directional face check defaults to
admitting the face.

The directional face cache is the block's liquid-detection mask, separate from
render face coverage. Its default packed value is `0x20000`, whose mask byte is
zero, matching the ordinary default cache. Special directional overrides and
liquid detection rules require their own established registrations.

Unknown material facts do not disable a horizontal contribution whose
below-neighbor liquid is absent: both fallback outcomes are zero in that case.
A required unknown directional or material fact returns no flow authority.
Water-like blocks without proven raw LiquidDepth, including bubble columns, also
remain unavailable. No fluid-height inverse supplies those facts. This keeps
waterlogged special shapes, remaining materials, bubble-column forces and
non-player probes explicitly outside the completed ordinary current behavior.
