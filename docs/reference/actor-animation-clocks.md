# Native actor animation clocks

The mob walking fix preserves the pack's authored `anim_time_update` expression and
uses its result as the clip clock. The compiler previously discarded this field, so
procedural quadruped and chicken leg rotations read elapsed seconds through
`query.anim_time`, even though their packs request distance-driven phase. This was a
clock-input mismatch, independent of packet velocity or a leg-rotation constant.

This fixes that identified movement desynchronization. It does **not** close the
broader native actor-animation parity gate: ordinary actors still evaluate Molang
on fixed simulation ticks and interpolate completed bone poses. Vanilla instead
samples interpolated motion queries while applying the animation at render time.
These procedures differ for nonlinear bone expressions, including the cosine used
by walking legs. Start/loop delays, independent instances of a shared clip, and
specialized motion multipliers also remain incomplete; see [plan.md](../../plan.md).

## Vanilla rules

| Area | Rule |
| --- | --- |
| Authored clock | Retain `anim_time_update` in the skeletal definition and assign its evaluated result before applying the pose. |
| Default clock | Use `query.anim_time + query.delta_time` when no time expression is authored. |
| Clip lifecycle | Apply loop and hold handling to the assigned time; reset the clock and completion state when the player resets. |
| Controller transitions | Entering a controller state recursively resets its child animation players. |
| Walking input | Combine horizontal position displacement with horizontal dynamic render-offset displacement. |
| Motion queries | Expose modified distance and speed using render interpolation. |

## Time assignment and lifecycle

The definition constructor compiles the default expression
`query.anim_time + query.delta_time`. During pose application, vanilla places the
player's previous stored clip time in `query.anim_time`, evaluates the time
expression, and **assigns its result** to the stored time. It does not add that
result as a delta. Loop/hold handling then updates the stored time and exposes it
to bone-channel Molang through `query.anim_time`.

Vanilla uses these endpoint rules:

- Reaching or passing the declared length sets the player's finished flag. This
  flag is sticky until reset; reversing the clock below the length does not clear
  it.
- A looping clip wraps with `fmodf` only when its length is nonzero and its time
  is strictly greater than its length. Equality samples the endpoint. A
  zero-length looping procedural clip keeps its unbounded time.
- Hold-on-last-frame stores the smaller of assigned time and length.
- A one-shot skips pose/event application when time is strictly greater than
  length. Equality still applies the endpoint.
- The assigned expression result is not clamped to zero. Negative clocks remain
  negative, and the remainder operation is not Euclidean normalization.
- Effective blend below float epsilon returns before the clock advances. Its
  previous time and finished flag remain available when it resumes. Start and
  loop delays can also pause clock application.

Reset initializes clip time to zero, previous event time to minus one, finished
to false, and the start-delay field to its initialization sentinel. Entering a
controller state recursively resets its child players. An absolute expression
such as modified distance consequently restores the actor's movement phase on
its next application; it does not subtract a distance baseline for that state.

Cinnabar retains scripted clocks alongside each rig, evaluates their expressions
once before sampling the main and additional model geometries, and supplies the
same resulting time to their bone queries. Dormant zero-weight clocks persist.
Actor resets discard prior clocks before controller finished-condition checks,
and controller reentry uses the new entry tick. The current key of clip plus entry
tick still does not represent every independent native animation-player instance.
Clips without an authored update retain the existing elapsed-tick timing path;
their complete pause/render-time behavior is not claimed as newly verified parity.

## Motion inputs and render interpolation

The walk-animation component contains a base multiplier, previous speed, current
speed, accumulated modified distance, and current horizontal displacement. The
state-vector input uses X/Z displacement between current and previous positions;
dynamic render offsets add their own X/Z displacement. Vertical displacement does
not enter this walking input.

For ordinary moving actors, with horizontal displacement `d`, base multiplier
`b`, speed `s`, and accumulated distance `D`, the verified update is:

```text
previous_speed = s
s = 0.6 * s + b * min(1.6 * d, 0.4)
D = D + s
```

When displacement is zero, the target instead uses
`min(abs(wrapped_body_yaw_delta) * 0.02, 0.2)`. Passengers zero previous/current
speed and stop accumulating modified distance. Hurt/fire states can multiply the
base by `1.5`; jumping multiplies it by `0.35`. These specialized conditions are
not all implemented in Cinnabar's current motion model.

The native query getters use render interpolation fraction `alpha`:

```text
modified_distance_moved = D - (1 - alpha) * s
modified_move_speed = min(lerp(previous_speed, s, alpha), 1)
```

The speed getter applies an additional factor of `1.5` for baby actors. Its
interpolation fraction is distinct from clip animation time and frame delta.
Current Cinnabar ordinary-actor queries use tick motion values before the renderer
interpolates poses; replacing the missing authored clock therefore repairs the
phase input but does not implement these native render-time getters. No teleport distance cutoff
or teleport-specific native animation reset was established by this investigation.

## Pack witnesses and carrier rebuilds

[assets/vanilla-source.json](../../assets/vanilla-source.json) selects the pack
revision, hash, archive, and local cache root; do not copy those pins into animation
code. Within its `resource_pack` directory,
`animations/quadruped.animation.json` (`animation.quadruped.walk`) and
`animations/chicken.animation.json` (`animation.chicken.move`) both author
`anim_time_update: query.modified_distance_moved`, loop their procedural leg
channels, and read `query.anim_time` in leg rotation expressions. Neither needs a
positive keyframe duration to advance this procedural clock.

The carrier now includes the compiled expression index and validates that it
references a retained Molang expression. Its compatibility version is defined
once by `ENTITY_BLOB_VERSION` in
[crates/assets/src/entity.rs](../../crates/assets/src/entity.rs). Bumping that
version prevents a pre-fix catalog, which cannot retain authored clocks, from
silently loading with the repaired runtime. The optional field's serialization
default is useful for payload handling; it does not bypass the version check.

Run `make assets` after updating the compiler/runtime. The actor artwork and
equipment catalogs embed the entity carrier's hash and must rebuild with it;
automatic preparation includes that dependency in their cache identities. The
[Makefile](../../Makefile) owns the carrier path and compiler-input dependencies.
The required entity carrier must fail startup with its path and `make assets`
rebuild instruction if stale or invalid. Packs, compiled carriers, reference
executables, and comparison captures remain local artifacts and do not enter git.
