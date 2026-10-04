# Liquid-current admission and impulse

The reference is the canonical reconstructed 1.26.50.26 client and its matching
PE, SHA-256 `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
The older named 26.30 reconstruction supplies identities; current bodies and PE
data supply the behavior below. Cinnabar's implementation is independently
written from this evidence.

Current `LiquidPhysicsSystem::_liquidBlockFetch` is **RVA 0x0a5d5c40**. Its
adapter is **0x0a5e8d50**. Registration **0x072c5ec0** installs the flow-policy
writer, then this block fetch, then lava, water and head sensing. Only afterward
does it register player triggers and pose updates through **0x072bb110** and
**0x072b6020**. Consequently current acts on retained velocity before jumping,
swim steering, relative acceleration and collision resolution. It samples the
preceding pose box at the current feet, without sweeping by velocity.

The ordinary player's water probe shrinks X/Z by `0.001` and Y by `0.401`;
lava uses `0.1` and `0.4`. Inverted axes clamp to the original center. Material
gather **0x0a5dd330** traverses Y, then Z, then X, with a floored lower bound and
exclusive `floor(f32(upper + 1))` range end. It gathers resolved-liquid cells
using IConstBlockSource slot `+0x28`: the extra non-air block takes precedence,
with primary fallback when extra is air. This is the getter **0x0319e4f0** in
vtable `0x150161770`. When both probes contact their liquid, the fetch selects
lava for current force.

Local flow-policy writer **0x0a5e6f90** admits ordinary current when the prior
flying ability is clear. `FlyTriggerSystem` intent **0x06dcdf80**, registered
after swimming intent inside **0x072b6020**, and its action **0x06dce090** run
later. The input therefore retains the preceding flow allowance for live ticks
and replay, independently of the newly selected flight mode.

Flow additionally requires a collected raw `liquid_depth` greater than zero.
When all collected depths are zero, the fetch checks only the collection's
outer horizontal faces in Z-, X+, Z+, X- order. A neighboring resolved block
with matching material and nonzero raw depth admits current. The entry's native
boundary bits are `0x04`, `0x20`, `0x08` and `0x10`, respectively. Falling depths
still admit current; collapsing them to an effective depth zero too early would
incorrectly disable the force.

Once admitted, every collected cell contributes its vector from
**0x0395d2f0**, including source-depth-zero cells. That helper already normalizes
each cell's result. The fetch sums these vectors in its native Y/Z/X cell order,
normalizes the aggregate again, multiplies by the selected material's impulse,
and adds it to the retained velocity. The force does not grow with the number
of touching cells and is not an average of separately scaled impulses.

Both normalization steps compute native float squared length as
`(X² + Y²) + Z²`, then float square root, accepting lengths at least `0.0001`.
The matching PE confirms this grouping at VA `0x14395d72d` and `0x14395d793`
inside the cell helper and `0x14a5d6826` inside the aggregate; the reconstructed
expression's operand order differs. Matching PE values are:

| VA | Native float | Meaning |
| --- | ---: | --- |
| `0x15005b1dc` | `0.0000999999975` | Normalization threshold |
| `0x1500d3a40` | `0.0140000004` | Water impulse |
| `0x1500d3a3c` | `0.00350000011` | Lava impulse |
| `0x14fea4060` | `1` | Exclusive gather end addition |

`world/current.rs` implements this aggregate and admission. Its production
query retains the identities of body, boundary and per-cell vector reads; the
immutable collision snapshot forwards the same query for replay. Material reads
are memoized and bounded. A proved dry or still-water result returns an explicit
zero impulse. Missing raw liquid depth preserves material contact but supplies
no current authority when that depth is required by selected body cells,
boundary admission or a per-cell vector. Rendered liquid height does not replace
the raw state. A legacy world or a per-cell vector whose native obstruction facts
are unavailable also returns no current authority, preserving the incomplete
boundary instead of inventing a vector from collision geometry. Mounted ownership and
non-player/item-actor liquid probes remain outside this player implementation.

The integrated client passed a locked local build. Tests and the affected
verification suite were skipped, following the user's explicit instruction.
The user accepted flowing-water movement in the rebuilt macOS/Metal client
connected to the local BDS on 19132. This manual check covers the reported
current defect; it does not close the unsupported boundaries below.

## Per-cell direction and native obstruction facts

`world/flow.rs` implements current **0x0395d2f0**, identified by the named
26.30 `LiquidBlockBase::_getFlow` at **0x0ab193c0**. Production registrations
retain BREG `ModelStateField::LiquidDepth` without collapsing values 8–15.
The helper uses raw depth for the falling branch and effective depth zero for
those values when comparing neighboring levels.

The matching PE's planar facing bytes at **0x15013e0c7** are `2, 5, 3, 4`
(Z-, X+, Z+, X-). Its masks at **0x1502a38a8** are `1 << facing` and the
opposite-facing table at **0x1500e01e0** swaps each adjacent facing pair.
For a matching neighboring liquid, both primary blocks must admit the
corresponding face. The contribution is `(neighbor depth - current depth)`
times that direction. A rejected face follows the same fallback as an
unmatched liquid: when the primary neighbor's material does not block motion,
a matching liquid beneath it contributes
`(below-neighbor depth - current depth + 8)` times that direction.

The getters are intentionally distinct. Matching PE reads neighboring liquid
depth through slot **+0x28** at **0x14395d42a**, primary material's motion byte
through **+0x10** at **0x14395d451**, and below-neighbor depth through
**+0x28** at **0x14395d48c**. Directional face reads use primary **+0x10**
at **0x14395d535** and **0x14395d58e**, followed by BlockType's virtual
**+0x88** at **0x14395d568** and **0x14395d5bd**. The falling-wall checks
also read primary **+0x10**, at **0x14395d6cd** and **0x14395d703**.

For raw depths at least eight, a solid primary neighbor or its block one cell
above causes the horizontal result to be normalized first, adds Y = `-6`, and
normalizes again. The matching constant is **0x150068e7c**. Each arithmetic
step remains float; widening happens after the normalized vector is complete.

Native material setup **0x0379bed0** establishes byte **+3** (`blocksMotion`)
and byte **+5** (`isSolid`). Types 0, 5 and 6 have both clear; types 1, 13 and
23 have both set. The bindings below come from constructors, not PREG
passability, render opacity, collision boxes, or BREG face coverage.

| Binding | Current source association | Material type |
| --- | --- | ---: |
| Air | Native air material setup | 0 |
| Water / flowing water | Liquid material setup; raw BREG LiquidDepth | 5 |
| Lava / flowing lava | Liquid material setup; raw BREG LiquidDepth | 6 |
| Dirt | DirtBlock constructor **0x0a7b9820** | 1 |
| Grass | GrassBlockBase constructor **0x0712ab00** | 1 |
| Stone | registerBlock<StoneBlock> **0x0dfb6010** → **0x0a5b7aa0** | 23 |
| Sand | registerBlock<SandBlock> **0x0dfb97a0** → **0x08efbad0** | 23 |
| Gravel | Constructor **0x0712be60**, vtable **0x1502a5290** | 23 |
| Ice / packed ice | registerBlock<IceBlock> **0x0dfc49c0** → **0x071305c0** | 13 / 23 |

Gravel's vtable resolves its dust producer to **0x0712c030**, which selects
the matching PE's gravel particle identifier at **0x15064f6f4**. This binds
the otherwise unnamed constructor to GravelBlock. The listed class vtables'
directional slot +0x88 resolves to **0x00085800**, which returns true.
LiquidBlock's current constructor is **0x0395a220**; its vtable
**0x1501905d0** and LiquidBlockBase's **0x150190a00** share that default.

The face cache at Block **+0xb9** is the packed
BlockLiquidDetectionComponent mask, separate from render face coverage.
Named 26.30 `BlockComponentDirectData::_finalizeInit` **0x0b15f8e0** places
that component in direct data at Block **+0xb8**. The current component
initializer **0x0ab6b560** initializes the packed value to `0x20000`, whose
mask byte is zero, matching the ordinary default cache. Special directional
overrides and liquid detection rules require their own source-established
registrations.

Unknown material facts do not disable a horizontal contribution whose
below-neighbor liquid is absent: both native fallback outcomes are zero in
that case. A required unknown directional or material fact returns no flow
authority. Water-like blocks without proven raw LiquidDepth, including bubble
columns, also remain unavailable. No fluid-height inverse supplies those
facts. This keeps waterlogged special shapes, remaining native materials,
bubble-column forces and non-player probes explicitly outside the completed
ordinary current behavior.
