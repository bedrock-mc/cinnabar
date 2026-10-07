# Liquid movement reference

The movement target comes from `assets/bedrock-target.json`; this reference
describes the vanilla **1.26.50.26** client. Implementation and tests are
independently written.

## Water contact and production data

Vanilla liquid physics forms the water-contact AABB by shrinking the horizontal
axes by `0.001` and the vertical axis by `0.401`. Lava uses `0.1` and `0.4`. A
shrink that would invert an axis clamps it to the original center. The
material-cell scan floors the lower bound and includes cells whose integer
coordinate is at most the upper bound. Ordinary contact does not compare the
actor with the rendered liquid surface. Cinnabar's existing
`simulator/environment.rs` implements this contact rule.

Liquid contact precedes this tick's pose change. Vanilla fetches liquid blocks,
then runs lava, water and head sensing, before the player swim trigger and pose
updates. The fetch reads the current AABB and does not sweep it by velocity. The
resulting was-in-water state then supplies jumping and travel selection after
the new collision height has been applied.

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
sensing: it reads the extra (secondary) block and falls back to the primary
block when the extra block is `minecraft:air`. Existing all-layer liquid flags
cover ordinary primary liquid and waterlogged secondary water. Exact priority
for a non-air secondary block that conflicts with a primary liquid remains a
boundary of this model.

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

## Water forces

Water travel acceleration blends the water movement attribute toward the
effective ground movement attribute by capped Depth Strider level divided by its
maximum level. The effective level is halved while airborne. Cinnabar's existing
`water_travel_speed` already implements the ordinary-player branch of this rule.

Water drag acts on retained velocity after movement. Its baseline horizontal
retention is `0.9` while the actor's sprint flag is set, independently of
swimming pose. Otherwise it uses an optional water-movement override or the
default `0.8`. Depth Strider blends horizontal retention toward `0.546000063`.
Vertical retention is always the independent default water value `0.800000012`;
enchantment does not change it.

Player water gravity installs `-0.005` (`0xbba3d70a`) only when actor flag 57,
swimming, is clear. A swimmer has no ordinary water gravity. Levitation uses a
separate travel path.

The ordinary held liquid jump adds `0.0399999991` to vertical velocity before
collision resolution, using float addition.

In water, held sneak/descent input adds `-0.0399999991` to vertical velocity.
Flying ability suppresses this water sink force. Ordinary water sneak therefore
changes vertical motion before movement, in addition to reducing horizontal
input.

Swimming steering reads the negative-pitch sine table and moves vertical
velocity toward that target using rate `0.0599999987`, or `0.0850000009` when
the target is below `-0.200000003`. Steering does not run while jump is held;
held jump therefore bypasses pitch steering and enters the jump step. All
steering products and addition are float operations. Taking the sine table at
negative pitch is distinct from negating the positive-pitch lookup at
non-cardinal angles.

For ordinary swimming, an upward sine target is allowed only while the primary
material cell at attach location 7 carries the liquid flag. Otherwise steering
sets vertical velocity to zero, including an already downward velocity. Diving
and level-look steering do not require that flag. This guard is implemented when
the caller supplies the tick-captured `liquid_attach_height`.

Attach point and material semantics:

- A player's third absolute attach offset is zero, and bounding-box size updates
  copy it into the actor's offsets.
- Attach location 7 reads exactly that offset. For an ordinary unmounted player
  the point is the player anchor minus the vanilla eye offset. The caller selects
  interpolation zero, retaining the previous pose offset rather than the
  camera's render-frame interpolation.
- The bounding-box input update floors this point and records whether its
  material is a liquid. That flag is set for water and lava and clear for air
  and ordinary solids.
- The material lookup reads the default (primary) block's material. It does not
  scan secondary liquid storage or compare fluid height.

Cinnabar records the existing local eye-offset state's height before simulation
in the movement input, so prediction history retains the queried point instead
of recomputing it from later pose state. A missing height leaves the guard
unspecified for legacy traces. The previous-offset selection matches vanilla;
the complete eye-offset transition and its replay still need a controlled
client comparison. Start/continue/stop and tick order are recorded in
[swimming-trigger.md](swimming-trigger.md).

| Value | Role |
| --- | --- |
| `0.899999976` | Sprint water horizontal drag |
| `0.800000012` | Water vertical/default horizontal drag |
| `0.546000063` | Depth Strider horizontal drag target |
| `0.0599999987` | Ordinary swim steering rate |
| `0.0850000009` | Diving swim steering rate |
| `-0.200000003` | Dive steering threshold |
| `0.0399999991` | Default held liquid jump |
| `-0.0399999991` | Held water descent |

The regressions in `crates/sim/tests/liquid_native.rs` freeze float outputs
derived from these rules in a synthetic submerged world. They cover sprint drag
without a swimming pose, independent vertical retention with Depth Strider,
jump ascent independent of look pitch, and dive steering followed by water drag
without gravity. They are numeric regressions, not recorded vanilla trajectories
or server-acceptance evidence. The surface regressions also cover the exact
floored attach-cell boundary, material contact above a shallow rendered liquid
surface, primary air with secondary water, and downward steering with a dry
attach point.

## Swim blend and jump suppression

The swim-amount writer saves the preceding amount and adds `0.100000001` while
actor flag 57 (swimming) or flag 114 (crawling) is set, capped at one. Otherwise
it adds `-0.100000001`, floored at zero.

The writer runs before this tick's swim trigger and jump step. The ordering
matters at both transition edges.

The jump step returns before all ordinary liquid/ground jump paths when
`0 < SwimAmount < 1`, or when swimming is set but the head is not in water. If
the actor was in water, this return also zeroes retained vertical velocity. Dry
partial-blend ticks retain that velocity and suppress the ground-jump request.
Head-in-water uses the primary-material/level comparison documented in
swimming-trigger.md, not the steering guard's broader water-or-lava material
flag.

Cinnabar retains `PlayerState.swim_amount` and `swim_pose_active` in prediction
history. The writer reads the preceding pose flag, then the current input
updates that retained flag for the following tick. First swim entry therefore
still sees amount zero; the following partial-blend ticks suppress jump.
First stop still sees the preceding full amount, before later ticks decay.
The jump regression freezes all float values through entry and exit,
distinguishes wet and dry suppression, exercises head sensing, and verifies
that correction replay produces the same gate.

## Liquid ledge exit and retained low pose

Climbing out of liquid applies while the actor travels in a liquid and collided
horizontally; it applies to water and lava independently of swimming/crawling
pose.

After collision and liquid drag/gravity, it translates the actual resolved
AABB by retained X/Z velocity and by
`(starting_y - current_y) + 0.600000024 + retained_velocity_y` vertically, with
each subtraction/addition and face translation in float. It first checks primary
liquid material, then collision shapes. If both probes are clear, retained
vertical velocity becomes float bits `0x3e99999a` (`0.300000012`); current
position is unchanged until the next tick.

The liquid test floors minimum faces, ceils exclusive maximum faces, reads the
primary block's material, and does not compare rendered liquid height or inspect
secondary storage. The collision test fetches collision shapes over the same
box.

The simulator now shares this bounded, identity-preserving probe between
ordinary low/standing liquid travel and swimming travel, using the collision
resolver's actual AABB. Previously the walking probe rebuilt a standing box and
swimming never received the escape impulse. The dedicated regression checks a
low pose below an overhead obstruction, both liquids, primary versus secondary
material, unavailable/conflicting probe rollback, and the float impulse. Dry
retained swimming uses ordinary acceleration and drag in its low box: travel
type selects water travel by whether the actor was in water, not by actor flag
57. These synthetic checks do not establish live server acceptance.

## Remaining parity boundaries

- The ordinary no-flight surface guard is implemented. Flying swimmers use
  separate ability-dependent gating and a different rate.
- The ordinary swim-transition/head-water jump gates are implemented.
  Jump-controller and swimmer-specific paths remain separate. Their one-shot
  alternate liquid impulse is `0.0280000009`. Those branches are not modeled by
  the ordinary fully submerged regression.
- A swim speed multiplier above one and the dolphin effect alter water
  acceleration; an optional water-movement override alters unsprinted drag.
  Those paths are not included in Cinnabar's input contract.
- Ordinary player liquid-current admission, per-cell flow and aggregate force
  are implemented, with replay-captured preceding contact pose and flying
  policy. The rules and exact material getter boundaries are recorded in
  [liquid-currents.md](liquid-currents.md). Compile and live acceptance remain
  pending for this integration. Directional obstruction facts that have not been
  established remain explicitly unavailable; mounted ownership and non-player
  liquid probes are separate paths.
- This investigation does not close the full liquid parity gate. Swim-entry/exit
  timing, surface behavior, bubble-column forces and waterlogged obstacles still
  need controlled vanilla and real-server comparisons.
