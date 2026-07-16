# Dual-Stage Actor Interpolation Design

**Scope:** Preserve remote `MovePlayer` movement metadata, add the Bedrock-style
three-tick player position stage, replace packet-time render delay with per-frame
partial-tick interpolation, and cull remote players before instance upload. Atmosphere,
chunk visibility, and world-publication scheduling are not part of this tranche.

## Data flow

Protocol normalization retains `head_yaw`, `on_ground`, teleport mode, and the signed
wire tick. The network sequencer maps a foreign `MovePlayer` to `ActorMoveEvent`
without manufacturing yaw, ground, teleport, or tick values. FIFO sequence remains
the movement-event identity; it is not treated as a simulation tick.

Client-world stores three poses for each tracked actor: received target, previous
simulation tick, and current simulation tick. Ordinary player movement updates the
target and resets a three-tick countdown. Each 20 Hz tick advances position by
`(target - current) / remaining_ticks`, copies the target rotation into the current
tick, and decrements the countdown. Teleports and accepted spawn replacements set all
three poses to the destination immediately. Non-player actors retain immediate
movement until their rendering tranche defines mob interpolation.

The app accumulates `Time<Real>` in 50 ms units, advances client-world by the resulting
whole ticks, and passes only the remaining fraction to render. Session replacement
clears the accumulator. Render samples between the adjacent client-world tick poses,
using shortest-path angle interpolation. It does not retain packet timestamps or add
the old 100 ms delay.

## Visibility and bounds

Render rejects non-finite sources, sorts and deduplicates runtime IDs, then filters
actors beyond 192 blocks or wholly outside the camera clip volume using a conservative
player-sized bounding box. It truncates the surviving list to 128 actors, builds only
those skin layers and instances, and retains the existing single instanced draw.
Missing or malformed camera data fails open to the finite/distance-bounded list.

## Lifecycle semantics

Runtime-ID replacement, unique-ID replacement, remove/re-add, dimension reset, and
session reset discard pending targets and initialize adjacent tick poses from the new
spawn. A teleport never frame-interpolates from the old location. Repeated publication
does not mutate tick state.

## Tests

RED tests cover exact `MovePlayer` field preservation, foreign-player conversion,
three-tick convergence and retargeting, same-frame packet collapse, immediate teleport,
replacement/reset behavior, the 20 Hz accumulator, partial-tick position/angle
sampling, frustum rejection, distance rejection, conservative edge inclusion, and the
128-instance cap. Focused crate suites plus `devtool verify-affected`, strict Clippy,
formatting, and whitespace checks gate the commit.
