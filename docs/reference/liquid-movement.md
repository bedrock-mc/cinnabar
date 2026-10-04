# Liquid movement reference

The movement target comes from `assets/bedrock-target.json`. This investigation
uses the canonical reconstructed **1.26.50.26 preview client**, Lens artifact 6,
and the matching executable SHA-256
`7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
The named 26.30 reconstruction identifies call meanings; every formula below
was checked in the current canonical body and matching PE data. The
reconstruction is derived evidence, not Mojang's original source. Implementation
and tests are independently written.

## Water contact and production data

Current liquid physics **RVA 0x0a5d5c40** forms the water-contact AABB by shrinking
the horizontal axes by `0.001` and the vertical axis by `0.401`. Lava uses `0.1`
and `0.4`. A shrink that would invert an axis clamps it to the original center.
The material-cell scan **0x0a5dd330** floors the lower bound and includes cells
whose integer coordinate is at most the upper bound. Ordinary contact does not
compare the actor with the rendered liquid surface. Cinnabar's existing
`simulator/environment.rs` implements this contact rule.

Liquid contact precedes this tick's pose change. Registration **0x072c5ec0**
installs `LiquidBlocksFetch` (current source `07.cpp`, line 470993; PE vtable
`0x150376d90`, tick **0x0a5e8d50**, callback **0x0a5d5c40**), then lava, water
and head sensing, before calling the player swim-trigger and pose registration.
The callback reads the current AABBShape and does not sweep it by StateVector
velocity. The resulting WasInWater component then supplies jumping and travel
selection after the new collision height has been applied.

Cinnabar retains `MovementInput.liquid_contact_height` from the preceding pose
and resamples that box at the current feet, including during correction replay.
The bounded block query covers both that box and the new movement sweep;
water/lava contact uses the preceding box, while collision and other body
effects use the new box. This matters at a shoreline: a low box may still sample
water after clamping its vertical inset to its center, while a newly standing
box's lower inset already samples the dry cell above it. Regressions cover both
entry and exit, new-pose collision bounds, replay and errors in cells reached
only by the preceding taller pose.

The body material getter has different layer semantics from head and ledge
sensing. PE **0x0a5dd330** calls IConstBlockSource slot `+0x28`; matching vtable
`0x150161770` selects **0x0319e4f0**, which reads the extra block through
`+0x20` and falls back to the primary block through `+0x10` when its type is air.
Initialization **0x02e431b0** identifies the comparison value `0x1551c6d40` as
`minecraft:air`. Existing all-layer liquid flags cover ordinary primary liquid
and waterlogged secondary water. Exact priority for a non-air secondary block
that conflicts with a primary liquid remains a boundary of this model.

The checked-in target BREG and PREG were decoded independently from their
declared manifests during this investigation. Both hashes, the PREG payload
digest, its exact BREG binding, all record identities and full payload lengths
matched. Every state of water and flowing water carries WATER | PASSABLE, no
solid boxes, unit movement factors and a positive depth-dependent liquid height.
Bubble-column states also carry water. Lava and flowing lava carry LAVA |
PASSABLE. The committed carriers therefore contain liquid physics facts.

The production path preserves those facts:

- `PhysicsCollisionRegistries::from_assets` registers the PREG facts in separate
  sequential-ID and network-hash registries.
- `app/src/movement/runtime_system.rs` constructs `PaletteWorld` from the active
  collision store, dimension and matching network-ID registry.
- `PaletteWorld::runtime_ids_at` reads every decoded storage layer.
- `PaletteWorld::block_physics` publishes every layer's flags and factors.
- The simulator samples every layer for water and lava contact.

This is static carrier and code-path evidence. It does not prove that a
particular live player received the correct chunks or palette mode. Some older
application registry tests deliberately use a synthetic PREG with only PASSABLE
facts; those fixtures cannot establish production water movement.

## Proven water forces

Water travel acceleration **0x0dc3eeb0** blends the water movement attribute
toward the effective ground movement attribute by capped Depth Strider level
divided by its maximum level. The effective level is halved while airborne.
Cinnabar's existing `water_travel_speed` already implements the ordinary-player
branch of this rule.

Water drag **0x0320fc20**, identified through its current adapter's
`MobMovementDrag::tickApplyWaterDrag` signature, acts on retained velocity after
movement. Its baseline horizontal retention is `0.9` while the actor's sprint
flag is set, independently of swimming pose. Otherwise it reads optional
WaterMovement or the default `0.8`. Depth Strider blends horizontal retention
toward `0.546000063`. Vertical retention is always the independent default
water value `0.800000012`; enchantment does not change it.

The current player's water-gravity body **0x0322d5d0** installs `-0.005`
(`0xbba3d70a`) only when actor flag 57, swimming, is clear. A swimmer has no
ordinary water gravity. Levitation uses a separate native travel path.

The ordinary held liquid-jump branch in **0x0a5dc2e0** adds `0.0399999991`
to vertical velocity before collision resolution. Its default comes from PE
VA `0x1500b5374`. The branch retains native float addition.

WaterSinkInputSystem's current adapter **0x0dc47ac0** calls ticking wrapper
**0x0dc3db60**, whose callback is **0x0dc3db30**. With WasInWater present,
held sneak/descent input (MoveInput byte 0 bit 2 or byte `0x60` bit 3) adds
`-0.0399999991` from PE VA `0x150106adc` to vertical velocity. Flying ability
suppresses this water sink force. Ordinary water sneak therefore changes vertical
motion before movement, in addition to reducing horizontal input.

Swimming steering **0x09fd2140** reads the negative-pitch sine table and moves
vertical velocity toward that target using rate `0.0599999987`, or
`0.0850000009` when the target is below `-0.200000003`. Its current dispatcher
**0x09fdd550** excludes `MobIsJumpingFlagComponent`; held jump therefore bypasses
pitch steering and enters the jump system. All steering products and addition
are native float operations. Taking the sine table at negative pitch is distinct
from negating the positive-pitch lookup at non-cardinal angles.

For ordinary swimming, an upward sine target is allowed only while the primary
material cell at attach location 7 carries the liquid flag. Otherwise steering
sets vertical velocity to zero, including an already downward velocity. Diving
and level-look steering do not require that flag. This guard is implemented when
the caller supplies the tick-captured `liquid_attach_height`.

The current source chain establishes the point and material semantics:

- Player construction **0x001eba20** supplies a zero third absolute attach
  offset to **0x0284dd20**. Bounding-box size update **0x02c33d10**, and its
  single-entity counterpart **0x02c39d40**, copy that offset into OffsetsComponent
  bytes `0x38..0x40`.
- Attach-position calculation **0x03751a80** selects location 7; its base
  calculation **0x037517b0** reads precisely those three floats. For an ordinary
  unmounted player, they are zero and the point is the native player anchor
  minus VanillaOffset. The caller selects interpolation zero, retaining the
  previous pose offset rather than the camera's render-frame interpolation.
- Bounding-box input update **0x09eeeb70** floors this point and copies material
  byte 2 into PlayerInputRequest byte `0x21`. Material setup **0x0379bed0** sets
  that byte for water and lava and clears it for air and ordinary solids.
- BlockSource material lookup **0x0319fa10** reads the default block's material.
  Matching executable disassembly confirms the virtual default-block slot at
  `0x10`. The current BlockSource vtable at image VA `0x150161770` selects
  **0x0319e020**, which reads the default chunk storage through **0x037af620**.
  It does not scan secondary liquid storage or compare fluid height.

Cinnabar records the existing local eye-offset state's height before simulation
in the movement input, so prediction history retains the queried point instead
of recomputing it from later pose state. A missing height leaves the guard
unspecified for legacy traces. The native previous-offset selection is proven;
the complete eye-offset transition and its replay still need a controlled
client comparison. Native start/continue/stop and current registration order
are recorded in [swimming-trigger.md](swimming-trigger.md).

Relevant current PE float words were read through PE section mapping, with
image base `0x140000000`:

| Image VA | Value | Consumer |
| --- | --- | --- |
| `0x14ff9c370` | `0.899999976` | Sprint water horizontal drag |
| `0x15005ea28` | `0.800000012` | Water vertical/default horizontal drag |
| `0x150167298` | `0.546000063` | Depth Strider horizontal drag target |
| `0x150361800` | `0.0599999987` | Ordinary swim steering rate |
| `0x150361804` | `0.0850000009` | Diving swim steering rate |
| `0x14ffab668` | `-0.200000003` | Dive steering threshold |
| `0x1500b5374` | `0.0399999991` | Default held liquid jump |
| `0x150106adc` | `-0.0399999991` | Held water descent |

The regressions in `crates/sim/tests/liquid_native.rs` freeze float outputs
derived from these bodies and PE values in a synthetic submerged world. They
cover sprint drag without a swimming pose, independent vertical retention with
Depth Strider, jump ascent independent of look pitch, and dive steering followed
by water drag without gravity. They are numeric source-derived regressions,
not recorded vanilla trajectories or server-acceptance evidence.
The surface regressions also cover the exact floored attach-cell boundary,
material contact above a shallow rendered liquid surface, primary air with
secondary water, and downward steering with a dry attach point.

## Swim blend and jump suppression

CurrentSwimAmountSystem **0x099e64c0** saves the preceding amount and adds
native `0.100000001` while actor flag 57 (swimming) or flag 114 (crawling)
is set, capped at one. Otherwise it adds `-0.100000001`, floored at zero.
These constants are PE VAs `0x14ffab644` and `0x14ffab670`.

The writer runs before this tick's swim trigger and MobJumpSystem. This follows
the actual registration chain, rather than inferring an order from metadata:
**0x072ce0d0** registers CurrentSwimAmount, then invokes **0x072c5ec0**;
that invokes **0x072bb110**, which invokes **0x072b6020** to register the
swim trigger before registering MobJump. Collection **0x028e64f0** appends
each identity to its category's vector, and **0x028e8f00** traverses it forward.
Current `07.cpp` call sites are lines 476769, 481041, 473675, 462530,
459956 and 463035. The source ordering matters at both transition edges.

MobJumpSystem **0x0a5dc2e0** returns before all ordinary liquid/ground jump
paths when `0 < SwimAmount < 1`, or when swimming is set but the
ActorHeadInWater component is absent. If WasInWater is present, this return
also zeroes retained vertical velocity. Dry partial-blend ticks retain that
velocity and suppress the ground-jump request. The head component uses the
primary-material/level comparison documented in swimming-trigger.md, not
the steering guard's broader water-or-lava material boolean.

Cinnabar retains `PlayerState.swim_amount` and `swim_pose_active` in prediction
history. The writer reads the preceding pose flag, then the current input
updates that retained flag for the following tick. First swim entry therefore
still sees amount zero; the following partial-blend ticks suppress jump.
First stop still sees the preceding full amount, before later ticks decay.
The jump regression freezes all native float values through entry and exit,
distinguishes wet and dry suppression, exercises native head sensing, and
verifies that correction replay reconstructs the same gate.

## Liquid ledge exit and retained low pose

MobMovementClimbOutOfLiquid's current callback is **0x09004df0**, identified
by its current adapter in `09.cpp` line 49309 and the named older body
**0x06401d40**. Its filter requires LiquidTravelFlagComponent and
HorizontalCollisionFlagComponent; it applies to water and lava independently
of swimming/crawling pose.

After collision and liquid drag/gravity, it translates the actual resolved
AABB by retained X/Z velocity and by
`(starting_y - current_y) + 0.600000024 + retained_velocity_y` vertically.
Matching PE instructions perform each subtraction/addition and face translation
in float. The raise constant is at VA `0x14ffab698`. It first checks primary
liquid material, then collision shapes. If both probes are clear, retained
vertical velocity becomes float bits `0x3e99999a` (`0.300000012`); current
position is unchanged until the next tick.

The executable confirms BlockSource virtual slots `0x38` and `0xa0`.
The matching vtable at `0x150161770` selects **0x031a7a20**
(containsAnyLiquid, through its thunk) and **0x031a4560**
(fetchCollisionShapes, with boolean one). The liquid test floors minimum
faces, ceils exclusive maximum faces, reads primary getBlock material, and
does not compare rendered liquid height or inspect secondary storage.

The simulator now shares this bounded, identity-preserving probe between
ordinary low/standing liquid travel and swimming travel, using the collision
resolver's actual AABB. Previously the walking probe reconstructed a standing
box and swimming never received the escape impulse. The dedicated regression
checks a low pose below an overhead obstruction, both liquids, primary versus
secondary material, unavailable/conflicting probe rollback, and native float
impulse. Dry retained swimming uses ordinary acceleration and drag in its low
box: TravelTypeSensing **0x09fefcb0** selects WaterTravel by WasInWater, not by
actor flag 57. These synthetic checks do not establish live server acceptance.

## Remaining parity boundaries

- The ordinary no-flight surface guard is implemented. Flying swimmers use
  separate ability-dependent gating and a different rate.
- MobJumpSystem's ordinary swim-transition/head-water gates are implemented.
  Jump-controller and swimmer-specific paths remain separate. Its one-shot alternate liquid impulse is
  `0.0280000009` at PE VA `0x150376a10`. Those branches are not modeled by the
  ordinary fully submerged regression.
- `SwimSpeedMultiplier` above one and DolphinFlag alter water acceleration;
  optional WaterMovement alters unsprinted drag. Those component paths are not
  included in Cinnabar's input contract.
- Ordinary player liquid-current admission, per-cell flow and aggregate force
  are implemented from **0x0a5d5c40** and **0x0395d2f0**, with replay-captured
  preceding contact pose and flying policy. The source evidence and exact
  material getter boundaries are recorded in [liquid-currents.md](liquid-currents.md).
  Compile and live acceptance remain pending for this integration. Native
  directional obstruction facts that have not been established remain explicitly
  unavailable; mounted ownership and non-player liquid probes are separate paths.
- This investigation does not close the full liquid parity gate. Native
  swim-entry/exit timing, surface behavior, bubble-column forces and waterlogged
  obstacles still need controlled vanilla and real-server comparisons.
