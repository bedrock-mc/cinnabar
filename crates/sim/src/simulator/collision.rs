use crate::{
    Aabb, CollisionWorld, ProvenancedCollider, Vec3, WorldCollisionIdentity, WorldQueryError,
};

use super::{AxisCollisions, COLLISION_EPSILON, STEP_HEIGHT};

#[derive(Debug, Clone)]
pub(super) struct ResolvedMotion {
    pub aabb: Aabb,
    pub resolved: Vec3,
    pub position: Vec3,
    pub collisions: AxisCollisions,
    pub identity: WorldCollisionIdentity,
    pub stepped: bool,
    pub support: Option<[i32; 3]>,
}

pub(super) fn resolve_motion(
    world: &impl CollisionWorld,
    position: Vec3,
    velocity: Vec3,
    was_on_ground: bool,
    height: f64,
) -> Result<ResolvedMotion, WorldQueryError> {
    let start = Aabb::player_with_height_at(position, height);
    let colliders = bounded_collision_boxes(world, start.swept(velocity))?;
    let mut identity = colliders.identity;
    let (normal_box, normal) = resolve_axes_reverse(start, velocity, &colliders.value);
    let normal_horizontal_collision = normal.x != velocity.x || normal.z != velocity.z;
    let normal_y_collision = normal.y != velocity.y;
    let on_ground = was_on_ground || (normal_y_collision && velocity.y < 0.0);

    let (resolved_box, resolved, stepped, support) = if on_ground && normal_horizontal_collision {
        // As vanilla's step-up collision volume does, cover the raised path too.
        let envelope = bounded_collision_boxes(
            world,
            start.swept(Vec3::new(velocity.x, STEP_HEIGHT, velocity.z)),
        )?;
        identity = identity.merge(&envelope.identity)?;
        let (step_box, step) = resolve_step(start, velocity, &envelope.value);
        let step_query = bounded_collision_boxes(world, step_box)?;
        identity = identity.merge(&step_query.identity)?;
        let step_blocked = !step_query.value.is_empty();
        if !step_blocked && step.horizontal_length_squared() > normal.horizontal_length_squared() {
            (
                step_box,
                step,
                true,
                supporting_block(step_box, &envelope.value),
            )
        } else {
            (
                normal_box,
                normal,
                false,
                supporting_block(normal_box, &colliders.value),
            )
        }
    } else {
        (
            normal_box,
            normal,
            false,
            supporting_block(normal_box, &colliders.value),
        )
    };

    let end_position = Vec3::new(
        f64::from((resolved_box.min.x as f32 + resolved_box.max.x as f32) * 0.5),
        resolved_box.min.y,
        f64::from((resolved_box.min.z as f32 + resolved_box.max.z as f32) * 0.5),
    );
    Ok(ResolvedMotion {
        aabb: resolved_box,
        resolved,
        position: end_position,
        collisions: AxisCollisions {
            x: (velocity.x as f32 - resolved.x as f32).abs() > COLLISION_EPSILON as f32,
            y: (velocity.y as f32 - resolved.y as f32).abs() > COLLISION_EPSILON as f32,
            z: (velocity.z as f32 - resolved.z as f32).abs() > COLLISION_EPSILON as f32,
        },
        identity,
        stepped,
        support,
    })
}

fn bounded_collision_boxes(
    world: &impl CollisionWorld,
    query: Aabb,
) -> Result<crate::CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
    crate::world::validate_collision_query(query)?;
    world.collision_boxes_with_provenance(query)
}

/// Queries whether a bounded volume is occupied while preserving the exact
/// world identity returned for the probe.
pub(super) fn has_collision(
    world: &impl CollisionWorld,
    query: Aabb,
) -> Result<crate::CollisionQuery<bool>, WorldQueryError> {
    let colliders = bounded_collision_boxes(world, query)?;
    Ok(crate::CollisionQuery {
        value: colliders
            .value
            .into_iter()
            .any(|shape| shape.aabb.intersects(query)),
        identity: colliders.identity,
    })
}

pub(super) fn clip_sneak_edge(
    world: &impl CollisionWorld,
    position: Vec3,
    velocity: Vec3,
) -> Result<(Vec3, Option<WorldCollisionIdentity>), WorldQueryError> {
    const OFFSET: f64 = 0.05_f32 as f64;
    let full_player = Aabb::player_at(position);
    let player = Aabb::new(
        Vec3::new(
            f64::from(full_player.min.x as f32 + 0.025_f32),
            full_player.min.y,
            f64::from(full_player.min.z as f32 + 0.025_f32),
        ),
        Vec3::new(
            f64::from(full_player.max.x as f32 - 0.025_f32),
            full_player.max.y,
            f64::from(full_player.max.z as f32 - 0.025_f32),
        ),
    );
    crate::world::validate_collision_query(player.swept(velocity))?;
    let mut clipped = velocity;
    let mut identity: Option<WorldCollisionIdentity> = None;
    for axis in [0, 2] {
        while clipped[axis] != 0.0 {
            let mut probe = Vec3::new(0.0, -STEP_HEIGHT * 1.01, 0.0);
            probe[axis] = clipped[axis];
            let query = bounded_collision_boxes(world, player.translated(probe))?;
            identity = Some(match identity {
                None => query.identity,
                Some(previous) => previous.merge(&query.identity)?,
            });
            if !query.value.is_empty() {
                break;
            }
            clipped[axis] = reduce_toward_zero(clipped[axis], OFFSET);
        }
    }
    while clipped.x != 0.0 && clipped.z != 0.0 {
        let query = bounded_collision_boxes(
            world,
            player.translated(Vec3::new(clipped.x, -STEP_HEIGHT * 1.01, clipped.z)),
        )?;
        identity = Some(match identity {
            None => query.identity,
            Some(previous) => previous.merge(&query.identity)?,
        });
        if !query.value.is_empty() {
            break;
        }
        clipped.x = reduce_toward_zero(clipped.x, OFFSET);
        clipped.z = reduce_toward_zero(clipped.z, OFFSET);
    }
    Ok((clipped, identity))
}

fn reduce_toward_zero(value: f64, offset: f64) -> f64 {
    if value.abs() <= offset {
        0.0
    } else {
        f64::from(value as f32 - value.signum() as f32 * offset as f32)
    }
}

fn resolve_axes_reverse(
    start: Aabb,
    velocity: Vec3,
    colliders: &[ProvenancedCollider],
) -> (Aabb, Vec3) {
    let mut current = start;
    let mut resolved = Vec3::ZERO;
    for axis in [1, 0, 2] {
        let mut axis_velocity = Vec3::ZERO;
        axis_velocity[axis] = velocity[axis];
        for collider in colliders.iter().rev().copied() {
            axis_velocity = current.clip_against(collider.aabb, axis_velocity);
        }
        // Per-axis resolution moves only along `axis`. From a fully embedded
        // start `clip_against` returns a minimal-translation ejection on the
        // deepest axis; on a horizontal axis that would fabricate inputless
        // horizontal motion the server reads as a movement cheat. Vanilla only
        // shortens intended motion toward zero on each axis, so keep just this
        // axis's component and, horizontally, clamp it into the intended range.
        // The vertical axis keeps the ejection as the provisional embedded
        // push-out this recovery envelope still relies on.
        let mut moved = axis_velocity[axis];
        if axis != 1 {
            moved = clamp_toward_zero(moved, velocity[axis]);
        }
        let mut applied = Vec3::ZERO;
        applied[axis] = moved;
        current = current.translated(applied);
        resolved += applied;
    }
    (current, resolved)
}

/// Reduces `value` into the closed interval between zero and `limit`, so a clip
/// can only shorten intended motion, never create or reverse it.
fn clamp_toward_zero(value: f64, limit: f64) -> f64 {
    if limit >= 0.0 {
        value.clamp(0.0, limit)
    } else {
        value.clamp(limit, 0.0)
    }
}

fn resolve_step(start: Aabb, velocity: Vec3, colliders: &[ProvenancedCollider]) -> (Aabb, Vec3) {
    let mut current = start;
    let mut up = Vec3::new(0.0, STEP_HEIGHT, 0.0);
    for collider in colliders.iter().copied() {
        up = current.clip_against(collider.aabb, up);
    }
    current = current.translated(up);

    let mut horizontal = Vec3::ZERO;
    for axis in [0, 2] {
        let mut axis_velocity = Vec3::ZERO;
        axis_velocity[axis] = velocity[axis];
        for collider in colliders.iter().copied() {
            axis_velocity = current.clip_against(collider.aabb, axis_velocity);
        }
        current = current.translated(axis_velocity);
        horizontal += axis_velocity;
    }

    let mut down = up * -1.0;
    for collider in colliders.iter().copied() {
        down = current.clip_against(collider.aabb, down);
    }
    current = current.translated(down);
    (current, horizontal + up + down)
}

/// Selects the highest collider center below the feet plane, then the closest.
/// Exact ties retain the first shape; missing provenance stays unknown.
fn supporting_block(player: Aabb, colliders: &[ProvenancedCollider]) -> Option<[i32; 3]> {
    let center = |min: f64, max: f64| (max as f32 - min as f32) * 0.5_f32 + min as f32;
    let feet = [
        center(player.min.x, player.max.x),
        player.min.y as f32 - 0.2_f32,
        center(player.min.z, player.max.z),
    ];
    let mut nearest: Option<(&ProvenancedCollider, f32, f32)> = None;
    for collider in colliders {
        let bounds = collider.aabb;
        let dx = center(bounds.min.x, bounds.max.x) - feet[0];
        let dy = center(bounds.min.y, bounds.max.y) - feet[1];
        let dz = center(bounds.min.z, bounds.max.z) - feet[2];
        let gap = -dy;
        let distance = dz * dz + dy * dy + dx * dx;
        if gap >= 0.0
            && nearest.is_none_or(|(_, best_gap, best_distance)| {
                gap < best_gap || (gap == best_gap && distance < best_distance)
            })
        {
            nearest = Some((collider, gap, distance));
        }
    }
    nearest.and_then(|(collider, _, _)| collider.block)
}
