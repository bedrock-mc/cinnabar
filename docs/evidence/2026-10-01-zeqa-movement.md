# Zeqa movement correction audit — October 1, 2026

The client had several independent packet and replay defects. The fixes below are
based on vanilla 1.26.50.26 behavior. This audit
**does not close vanilla parity**: the capture is insufficient to prove the first
burst's collision geometry or the last burst's replay-fallback reason.

## Changes

- `app/src/runtime/world/control_apply.rs`: player prediction corrections preserve
  the current view. MovePlayer continues to apply its rotation.
- `app/src/movement.rs`: both live and replayed PosDelta use end-of-tick velocity;
  keyboard raw diagonals normalize and analogue axes stay zero; correction snaps preserve button history;
  replay rebuilds StartJumping from actual initiation and Jumping from held input.
- `crates/input/src/router.rs` and `action.rs`: keyboard direction buttons leave analogue axes empty at the input source.
- `crates/protocol/src/login.rs`, `login/latency_probe.rs`, world event/stream routing, and `app/src/runtime/network/session/latency_reply.rs`: latency replies preserve the received flag and use native wrapping timestamp conversion. Replies follow committed motion and prior queued inputs. A bounded retained reply and a simulation pause preserve ordering under backpressure.
- `crates/sim/src/aabb.rs`: use the full player half-width in collision queries instead of the bedsim-only horizontal inset.
- `app/src/movement/physics/timeline.rs`: zero-tick motion changes live velocity
  without inventing a replay timestamp. Nonzero motion keeps its server stamp.
- `app/src/movement/physics/correction.rs`: corrected retained samples reflect
  authoritative velocity, ground state and axis-collision state. In-session snaps preserve jump cooldown and held/pending input.
- `app/src/movement/encoding.rs`: Jumping follows processed held jump;
  StartJumping follows a consumed jump, rather than a raw press or whole jump arc.
- `app/src/movement/teleport_ack.rs`: MovePlayer teleport acknowledgement is always
  active. Unverified correction-snap and respawn routes remain opt-in.
- `app/src/movement/control_trace.rs`, `trace.rs`, `runtime/world.rs` and
  `correction_shape.rs`: unthrottled incoming movement fields, original-sent versus
  retained positions, local tick, and explicit replay-fallback errors.

## Regression coverage

Numeric-only fixtures come from the supplied October 1 trace. The rotation fixture
covers input 31395 and corrections 31391–31395. The motion fixture isolates the
vertical trajectory at 33332–33338 on a synthetic flat floor: omitted server velocity
is explicitly inferred from successive server positions, not presented as captured.
It checks the corrected anchor and replayed positions, velocity/ground handling,
unchanged tick numbering, and absence of a second impulse. This is not a full
collision-world or complete incoming-packet replay.

Tests also cover live/replayed PosDelta, held/released/airborne jump flags, a held
jump across a teleport, keyboard versus gamepad vectors, and actual world-stream
teleport acknowledgement wiring, including failed sends and retry. The collision regression checks support at the logged X coordinate on an explicitly synthetic block. Legacy bedsim fixture bytes remain unchanged; three water-ledge comparisons now name the native full-width contact expectation.
The failing regression commits precede production fixes. The external task report
records the exact remote rcheck command, exit status and tested commit.

## Additional current-client rules

Jumping: input update carries held processed jump into MoveInput bit
`0x10`; `fillInputPacket` maps it to wire bit 6. StartJumping:
Jump initiation checks canJump, performs the jump, then sets action `0x100`;
`setFromComponent` maps that action to wire bit 31.

MovePlayer mode 2: `Player::handleMovePlayerPacket` sets action
`0x40000000`; `setFromComponent` maps it to HandledTeleport bit 37.
The current frame path replaces position, motion, ground and AABB,
without clearing physical input. Live teleport handling resets spatial and fall state without resetting jump components; in-session snaps therefore preserve the cooldown.

## Required live retest

Run a release client with `RUST_MCBE_MOVEMENT_TRACE=1`. It now records both outbound
PAI and `rust-mcbe-movement-control-v1` incoming movement records without the old
250 ms limiter. Leave `RUST_MCBE_TELEPORT_ACK` unset for the default verified
MovePlayer route; setting it to 1 also enables provisional extra routes.

Repeat stationary hits, falling hits, held jump through a duel teleport, and the
first burst's wall/ledge contact. Capture collision volumes and chunk revisions
for any remaining floor disagreement, and retain any replay-fallback warning.
Compare the same scenarios against vanilla, including device mode and diagonal
movement. No live server connection was made during this task.

# Trace analysis

34 committed correction events, six detailed `kind="correct"` diagnostics. The latter share a global 250 ms limiter with teleports and SetActorMotion, so they are not a correction count. They also compare *current retained prediction after prior replay*, not the original transmitted PAI. Server positions in the table were parsed back to f32 before subtracting the original trace position.

All 34 control-application log records are below; the old log does not distinguish replay, confirmation and ignored-history outcomes at that site. Delta is server minus originally sent position. Delay is latest transmitted tick at receipt minus corrected tick. Cause labels describe the numeric evidence; they do not claim unavailable server-side reasons or map geometry.

| Tick | UTC applied | Delay | Delta X | Delta Y | Delta Z | Cause |
|---:|---|---:|---:|---:|---:|---|
| 31391 | 10:36:57.087 | 4 | +0.000000 | +0.687721 | -0.003174 | C: floor/contact mismatch; propagated through later knockback |
| 31392 | 10:36:57.137 | 3 | +0.000000 | +0.999992 | -0.007812 | C: floor/contact mismatch; propagated through later knockback |
| 31393 | 10:36:57.187 | 2 | +0.249878 | +0.999985 | -0.007812 | C: floor/contact mismatch; propagated through later knockback |
| 31394 | 10:36:57.245 | 2 | +0.249878 | +0.999992 | -0.007568 | C: floor/contact mismatch; propagated through later knockback |
| 31395 | 10:36:57.287 | 2 | +0.249878 | +0.999992 | -0.007568 | C: floor/contact mismatch; propagated through later knockback |
| 33147 | 10:38:25.324 | 5 | +0.134521 | +0.790245 | -0.333252 | K1: server impulse starts 33147, client 33148; replay reapplies it |
| 33148 | 10:38:25.507 | 7 | +0.106201 | +0.703842 | -0.283936 | K1: server impulse starts 33147, client 33148; replay reapplies it |
| 33149 | 10:38:25.507 | 6 | +0.080566 | +0.619171 | -0.239502 | K1: server impulse starts 33147, client 33148; replay reapplies it |
| 33150 | 10:38:25.507 | 5 | +0.056885 | +0.536194 | -0.198730 | K1: server impulse starts 33147, client 33148; replay reapplies it |
| 33151 | 10:38:25.507 | 4 | +0.035400 | +0.454872 | -0.161621 | K1: server impulse starts 33147, client 33148; replay reapplies it |
| 33153 | 10:38:25.635 | 4 | -0.136230 | -0.493164 | +0.236084 | K1: follow-on replay double impulse; rotation reset changes input basis |
| 33154 | 10:38:25.702 | 5 | -0.152344 | -0.569695 | +0.263916 | K1: follow-on replay double impulse; rotation reset changes input basis |
| 33155 | 10:38:25.702 | 4 | -0.167236 | -0.644707 | +0.289307 | K1: follow-on replay double impulse; rotation reset changes input basis |
| 33332 | 10:38:34.658 | 6 | +0.153076 | +0.400002 | +0.575439 | K2: server impulse starts 33332, client 33333 |
| 33333 | 10:38:34.658 | 5 | +0.028320 | +0.313599 | +0.457275 | K2: server impulse starts 33332, client 33333 |
| 33334 | 10:38:34.686 | 4 | -0.025879 | +0.228928 | +0.437744 | K2: server impulse starts 33332, client 33333 |
| 33335 | 10:38:34.761 | 5 | -0.075439 | +0.145950 | +0.420166 | K2: server impulse starts 33332, client 33333 |
| 33336 | 10:38:34.822 | 4 | -0.120605 | +0.064629 | +0.404297 | K2: server impulse starts 33332, client 33333 |
| 33337 | 10:38:34.928 | 6 | -0.161377 | -0.015060 | +0.389893 | K2: server impulse starts 33332, client 33333 |
| 33338 | 10:38:34.928 | 5 | -0.198730 | -0.093163 | +0.376465 | K2: server impulse starts 33332, client 33333 |
| 33418 | 10:38:38.895 | 4 | -0.209717 | +0.413704 | +0.305176 | K3: server impulse starts 33418, client 33419; replay reapplies it |
| 33419 | 10:38:38.960 | 4 | -0.275879 | +0.327301 | +0.356934 | K3: server impulse starts 33418, client 33419; replay reapplies it |
| 33420 | 10:38:39.027 | 4 | -0.352295 | +0.242630 | +0.494141 | K3: server impulse starts 33418, client 33419; replay reapplies it |
| 33421 | 10:38:39.201 | 7 | -0.421631 | +0.159653 | +0.618896 | K3: server impulse starts 33418, client 33419; replay reapplies it |
| 33422 | 10:38:39.211 | 6 | -0.484619 | +0.078331 | +0.732666 | K3: server impulse starts 33418, client 33419; replay reapplies it |
| 33423 | 10:38:39.261 | 6 | +0.127441 | -0.415062 | -0.207275 | K3: server impulse starts 33418, client 33419; replay reapplies it |
| 33581 | 10:38:47.152 | 4 | +0.358276 | -0.420006 | +0.045166 | J: post-teleport jump rejected; client jump arc vs server grounded |
| 33582 | 10:38:47.347 | 6 | +0.416260 | -0.753204 | +0.052490 | J: post-teleport jump rejected; client jump arc vs server grounded |
| 33583 | 10:38:47.347 | 5 | +0.436890 | -1.001343 | +0.055176 | J: post-teleport jump rejected; client jump arc vs server grounded |
| 33584 | 10:38:47.347 | 4 | +0.437988 | -1.166115 | +0.055420 | J: post-teleport jump rejected; client jump arc vs server grounded |
| 33585 | 10:38:47.430 | 4 | +0.429321 | -1.249191 | +0.057373 | J: post-teleport jump rejected; client jump arc vs server grounded |
| 33916 | 10:39:04.498 | 4 | +0.203613 | +0.400002 | +0.240967 | K4: server impulse starts 33916, client 33917 |
| 33921 | 10:39:04.826 | 5 | +0.460449 | +0.424446 | +0.519287 | K4: correction snapped timeline to current tick; server arc is ahead |
| 33922 | 10:39:04.844 | 4 | +0.419067 | +0.102356 | +0.472656 | K4: correction snapped timeline to current tick; server arc is ahead |

## Interpretation

- C (5 events): Client steps +0.5 at 31377 and31379 after the initial impulse, collides horizontally at 31382 (X 1974.700073), then descends. Server catches floor at network Y 103.62; client continues to network Y 102.62. Tick 31393 reaches a wall while the server advances farther. Its tiny diagnostic delta compares already-replayed prediction; its original packet is one block below the server. A verified geometry discrepancy is fixed: Cinnabar subtracted 1e-4 from each horizontal half extent; vanilla builds full-width faces and passes them unchanged through ActorMoveSystem, SweptMovement and clip. At the logged X, the old box ends at 1974.999973; the full-width box reaches 1975.000073 (native f32: 1975.000122). A synthetic support-block regression now catches the lost contact. The actual Zeqa collision geometry and chunk revisions were not captured, so this fixes a proven discrepancy without claiming it definitively caused these five corrections. Remaining f32/f64 rounding differences require separate investigation.
- K1 (8): Server Y at33147=sent33146Y+0.4, while sent33147Y uses falling velocity-0.390245. SetActorMotion is committed after33147 and applied33148. Hence initial vertical error0.790245=0.4-(-0.390245), not gravity-order drift. Server then follows0.3136,0.228928,0.145949,0.064630,... recurrence one tick ahead. Correction of33147 replays an overlay at33148 that applies0.4 again, so sent33153 is0.493164 too high until subsequent corrections fix it. Rotation also resets from50.7deg to0; original33153/54 movement thus points differently.
- K2 (7): Server Y33332=sent33331Y+0.4; client33332 remains on ground. Motion is committed after33332, applied33333. The subsequent vertical error sequence .4,.3136,.228928,.145949,.064630,-.015062,-.093161 is exactly one tick of the same0.4 impulse arc. Horizontal differences also include a Right input beginning33333 and ground/air acceleration on different ticks. No evidence for jump or gravity constant error here.
- K3 (6): Server33418Y=sent33417Y+0.4. Client33418 lands, moving-0.013699Y, then applies motion33419. Ground-vs-air acceleration amplifies horizontal discrepancy. Correction33418 replays the same motion at33419, making sent33423Y102.18682 instead of101.77176. Detailed33419 diagnostic shows retained .4 velocity where server has .3136, direct evidence of the double application.
- J (5): Held jump is active before a local MovePlayer at10:38:46.771 after33578. Client reanchors and resets raw-edge history/jump cooldown;33579 emits JumpPressedRaw/StartJumping/StartSprinting while the buttons are continuously held, then lands33580 and jumps33581(.42Y). Server remains grounded throughout33581-85. No HandledTeleport appears. Hypotheses are raw-edge/cooldown reset, jump flag semantics and missing teleport acknowledgement; the log does not contain MovePlayer mode/tick, server jump permission, or correction velocity. This does not prove which server-side condition rejected the jump.
- K4 (3): Server33916 uses0.4 motion while client33916 is grounded and applies it33917. After correction33916, next sent tick33921 has anchor+0.3136 rather than four replay steps: code replay fallback snapped old correction to current tick. Precise fallback error is not logged. Followup33921/22 server positions stay on the original impulse arc. A new impulse at33924 further complicates any comparison after that point.

## Rotation answer

Last MovePlayer before the first burst is10:36:51.534; next is10:37:01.257. There is none around10:36:57.187. Before corrections, trace 31395 has yaw/head184.95005798339844,pitch4.824835777282715,camera[.08598163,-.08410978,-.99274021]. After corrections31391-31393, trace 31396 has yaw/head/pitch0 and camera[8.742277657e-8,0,1]. The production PlayerMovementCorrection arm directly applies correction yaw/pitch in control_apply.rs. The reset is a client correction-path bug, not a duel-start MovePlayer. Every burst repeats the reset.

## Limits

The outbound trace has no correction delta/ground/rotation packet fields, no block collision volumes or revisions, and no unthrottled received packet stream. All logged SetActorMotion ticks are zero. JSON traces record transport success and have no timestamps, so packet commit ordering is available but network receipt-to-simulation delay cannot be measured. The log therefore proves the phase mismatch relative to server but cannot justify moving every zero-tick impulse earlier globally. The native latency handler is sequential with packet dispatch. Cinnabar's receive-task reply could overtake app-side motion application; the ordered-fence fix addresses that mismatch, but the capture does not show latency probes. No live server connection was used. Remote regression builds are recorded separately.


# Vanilla movement reference findings

## Confirmed packet fields

- **`pos_delta` is state velocity, not resolved displacement.** `LocalPlayer::sendInput` equivalent obtains `StateVectorComponent` and copies position from offsets 0/8 and velocity from offsets 0x18/0x20 into the payload. There is no position subtraction. End-of-tick simulation state is what is sent.
- `fillInputPacket` copies analog move from `MoveInputComponent` offsets 4/8; processed move from 0x24/0x28; interaction rotation from 0x34/0x38; camera orientation from 0x54/0x58/0x5c. These are distinct carriers.
- The sender takes `head_yaw` from `ActorHeadRotationComponent`, tick from `CurrentTickComponent`, and collision flags from `HorizontalCollisionFlagComponent` / `VerticalCollisionFlagComponent`. `setFromComponent` takes pitch/yaw from `PlayerActionComponent` offsets 0x168/0x16c.
- **Keyboard raw movement is normalized too.** When raw analog axes (`MoveInputComponent`+0x14/+0x18) are both zero, the sender derives a direction from digital buttons. That helper sums cardinal/diagonal directions and divides every nonzero vector by its length. W+D therefore produces magnitude-one components around 0.70710677 in `raw_move_vector`, not 1/1. When either analog raw axis is nonzero, those raw axes are copied unchanged. This is an explicit analog-versus-digital branch, not a generic normalization of every input device.
- **No motion acknowledgement bit is required by this authoritative writer.** `ClientAckServerData` is present in enum reflection, but is not set by the authoritative packet writer. The motion handler also does not set a player-input action. The repository's independent `refs/bedrock-docs/player-auth-input.md:305` says this is a legacy acknowledgement not sent in server-authoritative movement mode. Adding it on knockback would invent behavior.

Jumping/start-jump/collision details are described above.

## SetActorMotion: tagged replay versus immediate velocity

Current `LegacyClientNetworkHandler::handle(SetActorMotionPacket)`:

1. For a local replay actor and a **nonzero packet tick**, creates a position-delta replay object, calls `ReplayStateComponent::applyFrameCorrection` at that packet tick, and clears replay-state byte 1.
2. For **tick zero**, bypasses the replay object/history APIs and invokes the actor motion virtual immediately. It never substitutes a local receive tick.

Immediate motion writes only the incoming vector to StateVector velocity offsets
0x18/0x20. It does not call other functions or write history, input flags, ground
state or rotation.

Therefore a zero-tick motion replaces live velocity once and affects the next simulation step. Recording it as a replay overlay at a synthesized local receive tick is incorrect: a later prediction correction that already incorporates that impulse will replay the impulse again. This directly supports the zero-tick overlay fix. A nonzero server tick retains replay semantics.

## CorrectPlayerMovePrediction

- The packet handler selects the rewind actor and calls the validity helper. That helper requires a nonzero packet tick, replay state with history, and tick at least the oldest retained history tick. It contains **no future upper bound** and **no latest-correction monotonic guard**.
- Wrapper passes the packet tick into `applyFrameCorrection`. Its underlying `_applyCorrection` attaches corrections to the next frame (`packet_tick + 1`) for replay. The corrected state is the server's state after the named tick; subsequent buffered inputs run from that state. This does not reset outgoing tick numbering.
- `CorrectPlayerPredictionInput::advanceFrame` equivalent sets on-ground, position, velocity, and AABB. It only changes rotation when prediction type is **1 (vehicle)**. Ordinary player correction therefore must preserve the player's view. The corresponding live-frame method is a no-op; the state changes are made through the replay frame path.
- `getAdvanceFrameResult` equivalent requests replay if on-ground differs, squared position error exceeds the tolerance, or squared velocity error exceeds the same tolerance. Vehicle rotation has an additional comparison. The exact current float tolerance is **9.999999747378752e-6** (float32 1e-5, bytes `ac c5 27 37`). It is a **squared** tolerance, not a 1e-5 per-axis distance.
- Thus a ~1e-4 position discrepancy alone does not establish why the server sent a correction, or whether vanilla would replay it. Velocity and on-ground are also compared. The log omits some of these server fields.

## Boundaries and unresolved points

- Current vanilla does not unconditionally reject future correction ticks at the front door. `applyFrameCorrection` routes missing/future frames through `_applyCorrection`; its absent-frame path can attach the correction to the current history frame. Exact future-frame behavior was not fully transferred into a Cinnabar design or tested by this trace. Report Cinnabar's existing future/stale ordering policy as a remaining parity difference, not as a vanilla rule.
- No evidence that the supplied corrections themselves legitimately reset player rotation; the player correction path does not apply packet rotation. A separate `MovePlayer` can legitimately set view, so the original log alone must still be checked for such a packet.
- Teleport held-input/cooldown preservation applies to both live and replayed frames, as described above.
- Exact server correction causality cannot be recovered from missing inbound fields, collision geometry, or an unobserved anticheat decision. These reference findings establish client defects; they do not prove all 34 corrections disappear without a live comparison.

## Keyboard analogue source

Keyboard direction buttons bind to bits 13–16. Their callbacks only modify
those bits; a separate analogue callback writes analogue axes. Input processing
preserves those axes and sends them unchanged. Pure keyboard input therefore
sends zero analogue axes, with a normalized digital raw-vector fallback.

## Latency probe reply

The latency handler copies the timestamp and from-server flag into its reply.
The reader multiplies the incoming timestamp by one million with fixed-width
arithmetic; the writer emits the stored value
unchanged. Cinnabar incorrectly cleared the flag and saturated overflow. Neither
latency packets nor their replies were present in the movement trace, so their
contribution to Zeqa's correction decisions cannot be measured from this log.

Receive, decode and packet handler invocation complete before the next packet
is dispatched. Latency and motion use the same handler object, so a preceding
zero-tick motion write completes
before the subsequent latency reply. Cinnabar's receive-task echo could run before
the app applied that motion. This is a confirmed ordering defect and a plausible
source of the one-tick phase mismatch, not proof of Zeqa's anticheat logic.
