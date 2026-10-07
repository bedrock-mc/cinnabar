# Flight control across spatial corrections

This describes the vanilla `1.26.50.26` client; the repository target game
version is selected by `assets/bedrock-target.json`.

## Vanilla behavior

- Advancing a movement frame changes ground state, position, motion and AABB.
  It does not clear movement abilities or input mode.
- The live-frame path handles the teleport route before advancing the live
  actor. Its spatial operations do not reset the flight trigger or movement
  abilities.
- The local flight state lives in the player input request and is toggled by
  input; the double-tap countdown is seven simulation ticks.
- Flight start/stop requests write boolean ability 9 and clear the flight
  countdown and fall distance.
- PlayerAuthInput maps the flight start/stop actions to wire input ordinals
  42/43.

The existing protocol carries those wire start/stop edges from the simulated
mode. Its `PosDelta` uses end-of-tick motion, matching the vanilla send path;
that lane does not require a flight-specific change.

## Fixed mismatch

An in-session snap called the same hard reanchor as a new session, which reset
the locomotion tracker to walking. A player who started flight locally before
receiving the server's flying ability acknowledgement therefore lost flight
after a spatial correction, and the next movement tick emitted `StopFlying`.

The snap now retains the locomotion tracker alongside its already-retained jump
input. It preserves both local flight state and the previous server flying
state, so a later authoritative flight clear is still detected. Ordinary
session and dimension reanchors retain their separate reset behavior.

The focused regression covers a locally initiated flight with no server
acknowledgement and a flight already acknowledged by the server. It applies
the snap through the transactional controller/ticker reconciliation, then
admits the next tick through the actual outbound ticker. That tick continues
flying without another start or a stop edge and retains the held vertical
control. A later authoritative flight clear still ends acknowledged flight
and produces its stop edge. The ticker's existing spatial correction retains
its previous held input; the snap's replay seed does not affect that path.

## Flight travel

Vertical fly speed control runs before movement:

- Idle horizontal input is maximum absolute processed axis below float `0.01`.
  Creative idle flight multiplies existing vertical motion by `0.375` only while
  neither vertical control is held.
- Held keyboard jump adds `0.15` times vertical fly speed; held keyboard sneak
  adds `-0.22` times that speed. Holding both clears vertical motion.
- Creative idle horizontal friction overrides the normal modifier with
  `0.375`; other idle flight uses `0.75`. Those modifiers affect horizontal drag.

Horizontal fly speed reads float ability 6 and a `[2, 1]` sprint multiplier
table. The keyboard vertical controls read float ability 7. Both custom speeds
and explicit zero are retained.

The ability defaults set protocol ability 13 (`FlySpeed`) to float `0.05`, and
ability 19 (`VerticalFlySpeed`) to float `1.0`. These match the fallback values
used by the flight helpers when the server omits a custom speed.

Fly drag uses friction coefficient `0.3999999761581421`. The float subtraction
from one produces vertical retention `0.6000000238418579`, independently of
horizontal hover friction. Horizontal drag multiplies the horizontal modifier by
ordinary air friction and clears each horizontal lane at float epsilon.

Flying travel uses the shared horizontal movement step. That step reads ground
contact before collision and selects the friction of the block at starting feet
minus float `0.1`. The player's travel settings enable this ground-friction
probe and leave vertical generic friction disabled. Flying therefore retains
horizontal motion by `ground friction * hover modifier * air friction`, while
separate flight drag retains vertical motion. Grounded flight now samples that
exact support coordinate with the ordinary query bound and world-identity
checks. Its regressions include hover, takeoff, landing and fractional support
height; takeoff and landing use the starting contact state for that tick's
friction.

The former flight path damped vertical motion after movement with the same
`0.91`-based horizontal retention. Creative idle damping also happened after
movement. The dedicated flight helpers apply creative idle damping before
collision resolution and retain the separate vertical drag afterward. Their
focused regressions freeze both movement and retained velocity for held
controls, hover, custom abilities and submerged flight.

## Vertical input lanes

The client sends raw ascend/descend input as wire `Ascend`/`Descend`, and the
slow vertical inputs as `WantDownSlow`/`WantUpSlow`. Keyboard jump or raw ascend
sets processed up, and keyboard sneak or raw descend sets processed down. Those
processed controls are sent as `WantUp`/`WantDown`, wire ordinals 16/17. The
extra raw-input acceleration lanes in the vertical controller correspond to the
distinct slow controls, which Cinnabar does not emit.

The server derives processed up/down directly from `WantUp`/`WantDown`. It reads
raw `Ascend`/`Descend` and `JumpDown`/`SneakDown` separately; those raw flags do
not populate processed up/down. The following input lock only clears restricted
raw inputs.

Cinnabar sent held jump and sneak, plus raw ascend/descend during flight, but
omitted processed `WantUp`/`WantDown`. The server therefore lacked the flight
vertical controls that the client simulated. The encoder now sends those
processed controls with held jump and sneak. The regression derives the server
control mask from the outbound flags for up, down, both and released controls.
The protocol regression verifies their named wire rows.

This correction does not establish full flight parity. The wall-time
double-tap detector remains an approximation of the vanilla tick countdown;
rapid-toggle acknowledgement ordering still needs independently identified
reconciliation behavior. Modified movement drag attributes and live server
acceptance remain separate evidence gates.
