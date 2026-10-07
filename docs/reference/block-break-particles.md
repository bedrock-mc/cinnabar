# Block destruction particles

The runtime pack is selected by `assets/vanilla-source.json`.

## Identified destruction path

The level-event dispatcher handles break events
2001 and 2021 by resolving the block, centering the emitter at the floored cell
plus 0.5, and reading the block's destruction-particle count, which is **100**
without a count override.
The previous
Cinnabar value, 32, did not match this path.

The terrain particle effect is `minecraft:block_destruct`, spawned with the count, its cube-root intensity, velocity scalar 1 and radius 0.5 for these
events. It refuses a request only when the selected effect already has more
than 20 emitters or more than 500 particles. These are strict pre-spawn checks,
not a clamp on the admitted burst, and do not count unrelated effects.

The destruction-texture getter first checks an explicit component
texture; its material fallback selects `down`, then `*`. The built-in texture
resolver ordinarily selects texture group zero. The block-graphics
loader and setter populate that group from `down`, not
`up`. Thus ordinary grass uses dirt pixels without a grass tint. There is no
reason to invent a green grass particle or choose a top face based on tint flags.

The pinned `particles/block_destruct.json` remains authoritative for piece size,
random quarter-tile sampling, random direction and speed, lifetime, gravity,
drag and collision. Those formulas are evaluated by the existing pack particle
engine rather than replaced with a hand-tuned visual effect.

## Implementation and verification

`particles/tiles.rs` selects the resolved bottom-face material and its tint.
`render::block_break_request` supplies the native default count and intensity.
`ParticleSystem::spawn_terrain` applies effect-local pre-spawn admission checks.
Regressions cover sequential and hashed grass IDs, untinted dirt selection,
100 live pieces from a valid effect, exact emitter/particle threshold boundaries,
and independence from other effects. Focused suites pass locally.
Fresh macOS/Metal Retina-scale-2 grass destruction frames show the dense brown
particle burst. The owner manually tested and accepted block breaking. This is
functional visual acceptance of the reported bug, not a matched native parity gate.

Incomplete: custom destruction-component count/texture overrides, the special
built-in block texture-group/crop branch, random block texture variants and
seasonal color overrides. Crack effects are a separate contract: native rendering selects a random point in the block's current visual bounds and moves
it outside the hit face, then requests count 1, velocity 0.7 and radius 0.
Upstream integration retains the vanilla one-piece/0.7/zero-radius crack
parameters and face placement; exact non-cube visual bounds and mining cadence
remain incomplete. The legacy Terrain event routing also needs independent parity
work. These gaps do not justify using the old 32-piece or green-top fallback
for ordinary grass destruction.
