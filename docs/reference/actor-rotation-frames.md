# Entity-relative bone rotation

Sneaking tilted the player's head because the animation compiler discarded the
bone's `relative_to.rotation` setting. The head then inherited the root's crouch
rotation during pose composition. The repair retains the authored frame in the
entity carrier and sampled bone pose, without a player-specific counter-rotation.

The pinned vanilla pack's `animation.humanoid.look_at_target.default` sets the
head's rotation relative to `entity` and reads the target pitch and yaw. The player
sneaking clip tilts the root while translating the body and head. Together these
records require the head's pivot to follow its parent while its axes use the
entity frame.

Vanilla first translates the bone through its parent transform, then resets the
inherited basis for entity-relative rotation before applying the bone's own
rotation and scale. This discards inherited rotation and scale at that boundary;
descendants inherit the resulting transform normally. Active clips retain the
greatest frame setting for each bone. An override restores the whole default pose
before its channels apply. The frame is resampled each tick and belongs to the
animation, rather than permanently to a geometry bone or a name such as `head`.

The compiler also preserves the setting on position-only and frame-only bones.
The entity carrier compatibility version changes to invalidate older catalogs;
rebuild with `make assets`. Artwork and equipment carriers also require rebuilding
because they retain the entity carrier hash. `ENTITY_BLOB_VERSION` is the single
version definition in the assets crate.

Regressions cover compiler/carrier retention, a parented pivot with an independent
rotation and scale, descendant inheritance, and standing/crouching/released player
look directions at level, upward, and downward angles. The rebuilt macOS Metal
client and dependent carriers were manually tested and accepted by the user.

This closes the identified sneaking head-rotation bug. Ordinary actor render-time
Molang evaluation, controller blend transitions, per-axis rotation objects, and
the broader actor-animation gates in [plan.md](../../plan.md) remain incomplete.
