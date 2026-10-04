# Swimming trigger and pose transitions

The target is the canonical reconstructed **1.26.50.26 preview client**, Lens
artifact 6, executable SHA-256
`7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
The older named 26.30 reconstruction identifies systems; formulas, current
dispatch and constants were checked in the current reconstruction and matching
PE. The code in Cinnabar is independently written.

## Current identities and ordering

`SwimTriggerSystem::doTick` is current **RVA 0x09fd25a0** (named older body
`0x053bf000`). Current ticking adapter **0x09fe1160** supplies this callback;
its assertion signature identifies the system and its component arguments.
Registration **0x072b6020** installs it through category-one registration,
before horizontal-pose and vanilla-offset updates.

Current category-one registration proceeds through **0x072ce0d0**, which
registers `CurrentSwimAmountSystem`, then calls **0x072c5ec0**. That registers
`InWaterSensingSystem` and `UnderWaterSensingSystem`, then calls **0x072bb110**.
The latter calls **0x072b6020** to register player input, swimming trigger and
pose updates before registering jumping and movement. The common registration
routine is **0x070be000**. The collection appends identities in **0x028e64f0**
and its ticker traverses forward in **0x028e8f00**.

Consequently body/head sensing reads the current position with the preceding
pose, and the swim-amount writer reads preceding swim/crawl flags before this
tick's start/stop action. Cinnabar now samples contact at the current position
before selecting its new pose; the former application path reused the preceding
tick's pre-movement contact after the position had already advanced.

## Native start and continuation

Entry requires the head-water component, flight disabled, swimming clear,
sprint intent (`PlayerInputRequest +0x08`) and clear sprint-direction rejection
(`+0x0a`). The direction rejection comes from current sprint intent
**0x0c5b6310**. Its ordinary desktop direction checks require movement magnitude
at least `sqrt(0.5)`, positive forward input and absolute sideways input at most
`sqrt(0.5)`. A separate native stall check compares retained position/input with
the current position, using `0.00005`; Cinnabar does not yet retain that complete
sprint-trigger state.

Entry additionally requires either a vertical look target below `0.15`, or both
the attach-seven material and the material one cell above the floored AABB
center to be non-air. Thus a strong upward look has an extra surface gate.

An existing swim continues while movement magnitude is at least `sqrt(0.5)`,
the hunger-stop request (`+0x10`) is clear, the player is unmounted (`+0x0b`),
the desktop/touch sprint cancellation (`+0x09`) is clear and body water contact
is present. This continuation does not require the actor's sprint flag or
positive forward input. The hunger request is produced in **0x0c5b1b60**:
missing hunger or hunger at most six requests a stop when flight permission is
absent. The application uses the existing shared hunger threshold.

While these continuation gates pass, non-air material at attach seven retains
the swim. For air at attach seven, native retains the swim when
`acos(horizontal_look_length_squared) * degrees_per_radian <= 45`, or the
vertical look target is nonpositive. The squared horizontal length is passed
to `acos`; interpreting this as a direct 45-degree pitch threshold changes the
surface behavior. The native lookup uses the float sine table for the negative
pitch and for `-yaw - pi`.

When continuation fails, native emits stop-swimming only if the standing fit
boolean (`+0x0c`) is true. There is no unconditional grounded stop in this
function. A blocked standing probe therefore retains swimming and its low box,
including on land; dry travel must still apply ordinary land/air forces.
Cinnabar formerly relabeled this condition as crawling and also ended a swim
whenever sprint or forward input cleared.

Current sprint intent **0x0c5b6310** skips its stop action when the preceding
swimming flag and current body-water flag are set. This preserves an existing
sprint through backward/sideways movement, sneak and released sprint input;
ordinary valid starts remain possible. Because sprint intent precedes swimming
trigger, even the first stop-swimming tick retains sprint while the old box is
wet. Cinnabar retains the actual flag with its controller frames, applies the
same selection during correction replay, and shares that flag across water drag,
wire flags and the runtime sprint attribute edge. A dry old box resumes the
ordinary sprint-stop conditions.

## Head sensing and standing fit

`UnderWaterSensingSystem::doUnderWaterSensing` is current **0x09fd7c00**,
dispatched by **0x09fd75f0**. It samples attach location seven at interpolation
zero, requiring the primary water material. Its strict eye comparison uses
`block_y + 1 - (level / 9 + offset)`, where `level = depth + 1` below depth
eight and one otherwise, and the offset is negative one-ninth. Source-water
head sensing therefore reaches the top of the cell, even though its rendered
surface is lower. It does not accept secondary-layer water as the head material.

PREG records store `(8 - depth) / 9` below depth eight and one otherwise
(`tools/registrygen/physics.go::liquidHeight`). The shared
`sim::sample_water_head` recovers that discrete level from the stored scalar and
then performs the native float comparison. The application passes its captured
pose eye height; it no longer probes every water layer at a fixed standing eye.
The same helper supplies jump sensing and preserves query identity.

Bounding-box input update **0x09eeeb70** writes the standing/sneaking/low fit
flags. Its standing probe uses the existing feet and standing collision height,
with all faces inset by `0.01`. `sim::pose_fits` now shares this rule; its old
vertical-only `0.001` inset disagreed near horizontal and ceiling contacts.

Pose-size transform **0x02c33550** sets horizontal-pose height to collision
width. Applying the request in **0x02c33600** preserves the previous AABB minimum
Y and changes maximum Y to minimum Y plus requested height. It does not shift
the feet or write StateVector Y. Vanilla offset affects the attach/camera point;
standing-up itself does not justify changing the network feet anchor.

## PE constants and remaining scope

| PE VA | Float | Consumer |
| --- | ---: | --- |
| `0x14feff2ac` | `0.707106769` | Swim and sprint movement threshold |
| `0x14ffab6c8` | `0.150000006` | Upward swim-entry target limit |
| `0x14ffd5070` | `57.2957764` | Native radians-to-degrees multiply |
| `0x15013adc8` | `45` | Surface continuation angle |
| `0x1503dcba0` | `0.0000499999987` | Sprint direction stall comparison |
| `0x150064950` | `9` | Head-water liquid-level divisor |
| `0x150167290` | `-0.111111112` | Head-water level offset |
| `0x1500c2b70` | `0.00999999978` | Fit probe lower-face inset |
| `0x1500dfa68` | `-0.00999999978` | Fit probe upper-face inset |

The focused regressions cover native source/flowing head heights, primary-layer
sensing, captured pose height, surface angle, backward/sideways continuation,
input and hunger stops, retained swim under a low ceiling, and current-position
contact. Touch sprint state, the complete stalled-entry state, mounted sensing,
rotation interpolation during trigger evaluation and replay of eye-offset
transitions remain incomplete. Bare synthetic collision worlds can omit primary
material identity; their optional air query cannot close a material parity gate.
The live palette adapter resolves exact primary air through its registry identity.
Immutable `CollisionSnapshot` forwards this material query to its retained
palette/registry, so correction replay evaluates the same surface gate after live
block replacement or eviction. The focused snapshot regression checks both the
material value and its retained collision identity.
This work does not close the complete liquid parity gate.
