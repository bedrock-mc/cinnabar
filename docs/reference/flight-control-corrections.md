# Flight control across spatial corrections

The analyzed reference is the canonical reconstruction of client `1.26.50.26`
in the private MCSRC workspace. The repository target game version is selected
by `assets/bedrock-target.json`; the available client reconstruction is a preview
build in that version family. Source bodies remain private and are not copied
into the repository.

## Identified native behavior

- `MovePlayerInput::advanceFrame`, current RVA `0x04b047e0`, changes ground state,
  position, motion and AABB. It does not clear movement abilities or input mode.
- `MovePlayerInput::advanceLiveFrame`, current RVA `0x04b046c0`, handles the
  teleport route before advancing the live actor. Its directly reconstructed
  spatial operations do not reset the flight trigger or movement abilities.
- `FlyTriggerSystem::doIntentTick`, current RVA `0x06dcdf80`, is identified by
  its full structural correspondence with the named reference counterpart
  `0x059aaf10`. The local flight state lives in the player input request and is
  toggled by input; the double-tap countdown is seven simulation ticks.
- `FlyTriggerSystem::doActionTick`, current RVA `0x06dce090`, corresponds to
  named reference `0x059ab030`. Flight start/stop requests write boolean ability
  9 and clear the flight countdown and fall distance.
- `PlayerAuthInputPacket::setFromComponent`, current RVA `0x0435a250`, maps the
  internal flight action bits 34/35 into wire input ordinals 42/43.

The existing protocol carries those wire start/stop edges from the simulated
mode. Its `PosDelta` uses end-of-tick motion, matching the native send path;
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

## Identified flight travel

The named older `VerticalFlySpeedControlSystem::doFlySpeedControlSystem`
counterpart identifies current RVA `0x02c452b0`. The current body and matching
executable float data establish these control operations before movement:

- Idle horizontal input is maximum absolute processed axis below the native
  float `0.01`. Creative idle flight multiplies existing vertical motion by
  `0.375` only while neither vertical control is held.
- Held keyboard jump adds `0.15` times vertical fly speed; held keyboard sneak
  adds `-0.22` times that speed. Holding both clears vertical motion.
- Creative idle horizontal friction overrides the normal modifier with
  `0.375`; other idle flight uses `0.75`. Those modifiers affect horizontal drag.

`HorizontalFlySpeedControl`, current RVA `0x03235360`, reads float ability 6 and
the matching executable's `[2, 1]` sprint multiplier table. The keyboard
vertical controls read float ability 7 in the vertical control body. Both
custom speeds and explicit zero are retained.

The current ability default constructor at RVA `0x001dd510` initializes
protocol ability 13 (`FlySpeed`) to native float `0.05`, and ability 19
(`VerticalFlySpeed`) to native float `1.0`. These match the fallback values
used by the flight helpers when the server omits a custom speed.

`FlyDrag`, current RVA `0x03217940`, reads friction coefficient
`0.3999999761581421`. The native float subtraction from one produces vertical
retention `0.6000000238418579`, independently of horizontal hover friction.
Horizontal drag, current RVA `0x03203a50`, multiplies the horizontal modifier
by ordinary air friction and clears each horizontal lane at float epsilon.

The flying travel wrapper at current RVA `0x099cce30` calls shared horizontal
movement at `0x099cc8b0`. That movement reads ground contact before collision
and selects the friction of the block at starting feet minus native float
`0.1`. The current `MobTravelComponent` constructor at `0x036ac750` enables
this ground-friction probe and leaves vertical generic friction disabled.
The flying friction wrapper at `0x0321f020` therefore retains horizontal
motion by `ground friction * hover modifier * air friction`, while separate
flight drag retains vertical motion. Grounded flight now samples that exact
support coordinate with the ordinary query bound and world-identity checks.
Its regressions include hover, takeoff, landing and fractional support height;
takeoff and landing use the starting contact state for that tick's friction.

The former flight path damped vertical motion after movement with the same
`0.91`-based horizontal retention. Creative idle damping also happened after
movement. The dedicated flight helpers apply creative idle damping before
collision resolution and retain the separate vertical drag afterward. Their
focused regressions freeze both movement and retained velocity for held
controls, hover, custom abilities and submerged flight.

## Vertical input lanes

The current send-input mapper at RVA `0x070fcfd0` maps raw input bits 17/18 to
wire `Ascend`/`Descend`, and raw bits 2/3 to wire
`WantDownSlow`/`WantUpSlow`. The current local input updater at RVA `0x07108cc0`
combines keyboard jump or raw ascend into processed up with mask `0x20080`,
and keyboard sneak or raw descend into processed down with mask `0x40001`.
Those processed controls are then sent as `WantUp`/`WantDown`, wire ordinals
16/17. The extra raw-input acceleration lanes in the native vertical
controller correspond to the distinct slow controls, which Cinnabar does not
emit.

The current server input handler at RVA `0x0998fe80` reconstructs processed
up/down directly from `WantUp`/`WantDown`. It reconstructs raw
`Ascend`/`Descend` and `JumpDown`/`SneakDown` separately; those raw flags do not
populate processed up/down. The handler is identified through the named
`ServerMoveInputHandlerSystemUtils` adapter at RVA `0x099abbb0`, whose entity
dispatch at `0x09990430` selects this body. The following input-lock operation
at `0x00480c40` only clears restricted raw inputs.

Cinnabar sent held jump and sneak, plus raw ascend/descend during flight, but
omitted processed `WantUp`/`WantDown`. The server therefore lacked the flight
vertical controls that the client simulated. The encoder now sends those
processed controls with held jump and sneak. The regression reconstructs the
native server control mask from the outbound flags for up, down, both and
released controls. The protocol regression verifies their named wire rows.

This correction does not establish full flight parity. The wall-time
double-tap detector remains an approximation of the native tick countdown;
rapid-toggle acknowledgement ordering still needs independently identified
reconciliation behavior. Modified movement drag attributes and live server
acceptance remain separate evidence gates.
