# Fish visibility and swimming animation

The pinned vanilla pack in `assets/vanilla-source.json` authors flat fins whose
unfolded box UV origin lies outside the texture. That origin does not describe
the faces with area. Rejecting the entire unfolded rectangle excluded cod,
salmon, the small pufferfish and tropical fish from the actor artwork carrier.

## Drawable fin UVs and tropical selection

Native Cube setup truncates authored box dimensions for the UV layout. With
zero X extent, the east/west rectangles cover U from `u` through `u + 2z`, and
V from `v + z` through `v + z + y`. With zero Y extent, the top/bottom rectangles
cover U from `u + z` through `u + z + 2x`, and V from `v` through `v + z`.
Unused faces have zero area. Cube and bone inflation can give them area, so
admission accounts for both. Genuine out-of-range drawn UVs remain rejected;
the existing front-only item-sprite exception is retained.

The native tropical-fish variable update tests the low byte of Int variant
metadata for the base family. Int mark-variant values outside zero through five
reset to zero; the B family adds six to the pattern. Wrong metadata tags use
the defaults. Cinnabar publishes Base/Pattern before authored scripts and render
controllers, allowing both body families and all twelve texture selections.

## Preventing false land-flop transitions

The pinned `controller.animation.fish.general` starts in `flopping`, switches
to `swimming` when `query.is_in_water || query.is_levitating`, and switches back
only when both are false. The fish scripts assign `variable.ZRot` up to 90
degrees only while dry. False dry samples therefore caused the large tilt and
subsequent upright snap the user observed.

Cinnabar previously sampled a point at `actor.position.y + 0.1` against the
fluid surface height and treated unreadable data as dry. Native LiquidPhysics
instead tests body-volume material contact, which InWaterSensing publishes to
the component read by `query.is_in_water`.

For ordinary actors, the water probe shrinks the AABB by `[0.001, 0.401, 0.001]`;
the lava probe uses `[0.1, 0.4, 0.1]`. An inverted axis collapses to its original
center, which matters for small fish. Material cells between the floored probe
minimum and inclusive maximum establish contact without comparing surface
height. Simultaneous water/lava contact selects lava. The shared Cinnabar probe
follows this contract and preserves the last valid actor sample when world data
is unavailable. The existing local-player liquid-contact arithmetic is shared
without changing its behavior.

Native item actors bypass the shrink, and additional sub-box handling exists.
Those branches and the complete unavailable-request lifetime remain open;
ordinary fish explain the reported animation issue.

## Restoring the swimming phase

Native FishAnimationSystem copies the current phase to previous, then advances
current by `1 + 0.1 * length(StateVector.velocity)` each tick. The native variable
updater publishes `variable.AnimationAmount` and `variable.AnimationAmountPrev`
before authored scripts. The pack interpolates those fields with
`query.frame_alpha` before evaluating lateral body and tail sway. Their former
absence froze that sway independently of the dry-state roll.

Cinnabar advances this retained phase once per elapsed simulation tick,
including ticks that skip pose evaluation. Reading a held frame consumes no
phase. The native phase component starts with both fields zero. Cinnabar retains
it across geometry/controller resets and resets it for a new actor lifetime.
Only cod, salmon, pufferfish and tropical fish receive these engine variables.

The existing actor velocity mixes displacement-derived blocks/second with raw
packet blocks/tick. Fish phase therefore uses a separate retained native vector.
Spawn, motion packets and the local physics feed supply raw per-tick velocity.
Native remote position interpolation clears this vector whenever its count is
positive at tick start, including the final count-one tick; otherwise it retains
the vector. Hard-position assignment preserves native velocity. Existing movement
queries keep their prior velocity source.

## Validation and remaining gates

Before integration with latest dev, the artwork regressions passed 7/7 and the
optional pinned-pack geometry/binding test passed. Tropical metadata tests cover
all twelve selections, exact Int tags, invalid values, refresh and actor isolation.
The complete client-world suite passed 214 tests with eight ignored, including
phase and packet-velocity regressions. The separate ignored pinned carrier pose
test passed for all four families: stationary wet fish change their authored tail
poses while their bodies remain upright. Body-probe regressions passed 7/7.
The architecture check and production client build also passed.

The user accepted the fresh macOS Metal client in vanilla rendering with ordinary
controls, after first confirming visibility and then confirming correct animation.
Its window content was 1280 by 720 logical pixels. It used the existing ocean BDS
world, without terrain changes. Task services were stopped for publication.
The user explicitly requested integrating latest dev, skipping further tests and
pushing directly to dev. Post-integration affected verification is therefore not
claimed. The earlier all-target clippy attempt encountered first-run stamp test
errors outside this change; latest dev includes their fix.

Full fish parity remains incomplete. Native rendering interpolates phase before
evaluating trigonometric channels; ordinary Cinnabar rigs sample frame alpha at
fixed ticks and interpolate bone poses. Exact intermediate-frame timing and the
complete movement/fish/render scheduler order remain open. Tropical fish also
need native two-sampler composition with independent base/pattern colors; the
A-family pattern-five raster contains fractional alpha outside the current
neutral binary-alpha carrier profile. This correction does not close those gates.
