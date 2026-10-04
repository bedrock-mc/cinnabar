# End crystal rendering and respawn anchor use

The pack reference is the install fetched from `assets/vanilla-source.json`, read
through this worktree’s `.local`. No pack art is shipped.

## End crystals

The pack's `entity/ender_crystal.entity.json` selects `ender_crystal` material,
`geometry.ender_crystal` and `animation.ender_crystal.move`. Its geometry contains
nested outerglass, innerglass and crystal cubes, plus a base. The animation
provides their rotation, inherited scale and bobbing; the render controller hides
only the base when `query.show_bottom` is false. These already compile and run
through the actor engine.

The artwork compiler rejected the entire texture because eight texels have alpha
127. Thus the rig had no artwork binding. The native material is alpha tested:
the installed PlayCover `data/resource_packs/vanilla/materials/entity.material`
lines 952, 174 and 56 specify `ender_crystal:entity_alphatest`, no culling and
point sampling. `data/shaders/glsl/entity.fragment` lines 56 and 82 discard below
0.5. The installed app's internal version is 1.26.51.01, as recorded in
[nametag-rendering.md](nametag-rendering.md); the local Android 1.26.31.1 material
and shader independently have the same contracts. These are adjacent-version
material evidence, not a matched native visual acceptance witness.

The compiler now bakes that coverage for rasters selected by the vanilla crystal
rig, retaining RGB and the existing binary-alpha carrier contract. It does not
change generic fractional-alpha admission or server-pack materials. Rebuild the
installed carriers with `make assets` before playing with this change; tests
compile scratch fixtures without writing the shared install.

The beam is separate native additional rendering, not part of the pack rig.
Metadata key 47 is a BlockPos with the zero sentinel.
Beam rendering reads
that target, skips zero, adds one to its Y coordinate and connects it to the
interpolated actor origin. Its tessellator makes eight tapered
sides, black at the target and white at the crystal. The constants are crystal radius 0.75, target radius ratio 0.2, eight sides and
normalization threshold 0.0001. The shader UV offset uses actor age plus partial
tick, times 0.01. The target offset vector is `[0,1,0,0]`.
The installed material's `entity_beam` uses vertex colour, alpha test,
repeat wrapping and UV animation, matching the existing solid effect pass.

The client now publishes this metadata-driven beam into the existing textured
effect pass, with runtime-loaded beam art, native endpoint interpolation, gradient
and UV scroll. Session/dimension transitions use the actor store's existing
lifetime clearing. No separate retained beam lifetime is introduced.

Beam submission shares the non-player body's camera-centred candidate-cube
admission. It uses the interpolated crystal endpoint
and the same render camera, including third-person camera displacement. A target
near the camera cannot admit an out-of-range crystal. Rejected beams consume no
effect submission capacity and generate no animated mesh or upload. Regression
tests first reproduced the orphaned beam consuming the last scene slot, then
verified all six cube faces, inclusive boundaries, corners and advancing ages.

## Respawn anchors

The fixed block-interaction list omitted respawn anchors. A held glowstone block
therefore selected placement and predicted an adjacent block instead of using
the clicked anchor.

Anchor interaction checks the selected block item for glowstone.
Below full charge it succeeds in every dimension, but consuming glowstone and
bumping charge occur only on the server. At full charge it proceeds to charged
activation, without consuming glowstone or increasing charge. Charged activation
sets the spawn in the Nether, without consuming charge; outside the
Nether it explodes on the server. Non-glowstone use of an uncharged
anchor falls through to ordinary item use. Sneaking with a held item bypasses
block interaction.

Local interaction selection now uses the held block identifier and anchor charge.
Glowstone selects interaction at every charge. Charged anchors also select
interaction for other items or an empty hand. No adjacent placement, charge,
inventory consumption or explosion is predicted; authoritative updates supply
those changes.

Block use sends
the ordinary standalone ItemUse InventoryTransaction for block use. The client
uses click-block action 0 (named `Place` by the wire enum), Success prediction,
the clicked anchor position/runtime ID, original held glowstone descriptor and
input/repeat trigger. It does not send a special charging packet.

Incomplete: non-glowstone use of an already-selected Nether spawn can fail. The client does not yet
retain spawn-block authority to predict that fallback. Glowstone still succeeds
on that path. The server remains authoritative for spawn and explosions.

## Verification boundary

Regression tests cover synthetic alpha cutoff boundaries, compiling installed
crystal art into a drawable binding, nested animation and show-bottom visibility,
beam metadata and tapered mesh/gradient/scroll, anchor charge levels, placement
suppression, sneaking and exact transaction fields. Tests using installed assets
skip when `.local` fixtures are absent.

Local verification passed, with every Cargo command routed through the owner's
`cslot`: `cargo check --locked -p bedrock-client --tests`, affected-library
`cargo check --tests`, and full tests for `assets`, `pack-compiler`, `client-world`,
`chunk-pipeline` and `render`. The application library suite passed 2,252 tests
with zero failures and 29 ignored. Formatting checks passed for all affected
crates. Installed artwork fixtures were present and exercised.

These are local code and test fixes. No owner account, public server or live
native comparison was used. A fresh target-platform rendered-frame comparison,
general actor material propagation and exact per-render-frame nonlinear Molang
evaluation remain incomplete. No complete crystal visual parity gate is closed.
