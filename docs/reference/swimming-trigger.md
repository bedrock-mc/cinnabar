# Swimming trigger and pose transitions

Target: vanilla **1.26.50.26** client. The code in Cinnabar is independently
written.

## Ordering

Each tick vanilla runs, in order: the swim-amount writer, body water sensing,
head water sensing, player input, the swimming trigger, then pose updates
(horizontal pose and vanilla offset), and finally jumping and movement.

Consequently body/head sensing reads the current position with the preceding
pose, and the swim-amount writer reads preceding swim/crawl flags before this
tick's start/stop action. Cinnabar now samples contact at the current position
before selecting its new pose; the former application path reused the preceding
tick's pre-movement contact after the position had already advanced.

## Start and continuation

Entry requires head water contact, flight disabled, swimming clear, sprint
intent and no sprint-direction rejection. The direction rejection comes from
sprint intent: ordinary desktop direction checks require movement magnitude at
least `sqrt(0.5)`, positive forward input and absolute sideways input at most
`sqrt(0.5)`. A separate stall check compares retained position/input with the
current position, using `0.00005`; Cinnabar does not yet retain that complete
sprint-trigger state.

Entry additionally requires either a vertical look target below `0.15`, or both
the attach-seven material and the material one cell above the floored AABB
center to be non-air. Thus a strong upward look has an extra surface gate.

An existing swim continues while movement magnitude is at least `sqrt(0.5)`,
no hunger stop is requested, the player is unmounted, the desktop/touch sprint
cancellation is clear and body water contact is present. This continuation does
not require the actor's sprint flag or positive forward input. Missing hunger or
hunger at most six requests a stop when flight permission is absent. The
application uses the existing shared hunger threshold.

While these continuation gates pass, non-air material at attach seven retains
the swim. For air at attach seven, vanilla retains the swim when
`acos(horizontal_look_length_squared) * degrees_per_radian <= 45`, or the
vertical look target is nonpositive. The squared horizontal length is passed
to `acos`; interpreting this as a direct 45-degree pitch threshold changes the
surface behavior. The look vector uses the float sine table for the negative
pitch and for `-yaw - pi`.

When continuation fails, vanilla emits stop-swimming only if the standing pose
fits. There is no unconditional grounded stop. A blocked standing probe
therefore retains swimming and its low box, including on land; dry travel must
still apply ordinary land/air forces. Cinnabar formerly relabeled this condition
as crawling and also ended a swim whenever sprint or forward input cleared.

Sprint intent skips its stop action when the preceding swimming flag and current
body-water flag are set. This preserves an existing sprint through
backward/sideways movement, sneak and released sprint input; ordinary valid
starts remain possible. Because sprint intent precedes the swimming trigger,
even the first stop-swimming tick retains sprint while the old box is wet.
Cinnabar retains the actual flag with its controller frames, applies the same
selection during correction replay, and shares that flag across water drag,
wire flags and the runtime sprint attribute edge. A dry old box resumes the
ordinary sprint-stop conditions.

## Head sensing and standing fit

Head sensing samples attach location seven at interpolation zero, requiring the
primary water material. Its strict eye comparison uses
`block_y + 1 - (level / 9 + offset)`, where `level = depth + 1` below depth
eight and one otherwise, and the offset is negative one-ninth. Source-water
head sensing therefore reaches the top of the cell, even though its rendered
surface is lower. It does not accept secondary-layer water as the head material.

PREG records store `(8 - depth) / 9` below depth eight and one otherwise
(`tools/registrygen/physics.go::liquidHeight`). The shared
`sim::sample_water_head` recovers that discrete level from the stored scalar and
then performs the vanilla float comparison. The application passes its captured
pose eye height; it no longer probes every water layer at a fixed standing eye.
The same helper supplies jump sensing and preserves query identity.

The bounding-box input update writes the standing/sneaking/low fit flags. Its
standing probe uses the existing feet and standing collision height, with all
faces inset by `0.01`. `sim::pose_fits` now shares this rule; its old
vertical-only `0.001` inset disagreed near horizontal and ceiling contacts.

The pose-size transform sets horizontal-pose height to collision width. Applying
the request preserves the previous AABB minimum Y and changes maximum Y to
minimum Y plus requested height. It does not shift the feet or write position Y. Vanilla offset affects the attach/camera point; standing-up itself does not
justify changing the network feet anchor.

## Constants and remaining scope

| Float | Role |
| ---: | --- |
| `0.707106769` | Minimum movement magnitude for swim entry, continuation and sprint direction |
| `0.150000006` | Vertical look target below which entry skips the surface gate |
| `57.2957764` | Radians-to-degrees multiply for the surface continuation angle |
| `45` | Surface continuation angle limit, in degrees |
| `0.0000499999987` | Sprint-direction stall comparison |
| `9` | Head-water liquid-level divisor |
| `-0.111111112` | Head-water level offset |
| `0.00999999978` | Fit probe lower-face inset |
| `-0.00999999978` | Fit probe upper-face inset |

The focused regressions cover source/flowing head heights, primary-layer
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
