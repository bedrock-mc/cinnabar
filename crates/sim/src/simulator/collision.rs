use crate::{Aabb, CollisionWorld, Vec3, WorldCollisionIdentity, WorldQueryError};

use super::{AxisCollisions, COLLISION_EPSILON, STEP_HEIGHT};

#[derive(Debug, Clone)]
pub(super) struct ResolvedMotion {
    pub resolved: Vec3,
    pub collisions: AxisCollisions,
    pub identity: WorldCollisionIdentity,
    pub stepped: bool,
}

pub(super) fn resolve_motion(
    world: &impl CollisionWorld,
    position: Vec3,
    velocity: Vec3,
    was_on_ground: bool,
) -> Result<ResolvedMotion, WorldQueryError> {
    let start = Aabb::player_at(position);
    let colliders = bounded_collision_boxes(world, start.swept(velocity))?;
    let mut identity = colliders.identity;
    let (normal_box, normal) = resolve_axes_reverse(start, velocity, &colliders.value);
    let normal_horizontal_collision = normal.x != velocity.x || normal.z != velocity.z;
    let normal_y_collision = normal.y != velocity.y;
    let on_ground = was_on_ground || (normal_y_collision && velocity.y < 0.0);

    let (resolved_box, stepped) = if on_ground && normal_horizontal_collision {
        let (step_box, step) = resolve_step(start, velocity, &colliders.value);
        let step_query = bounded_collision_boxes(world, step_box)?;
        identity = identity.merge(&step_query.identity)?;
        let step_blocked = !step_query.value.is_empty();
        if !step_blocked && step.horizontal_length_squared() > normal.horizontal_length_squared() {
            (step_box, true)
        } else {
            (normal_box, false)
        }
    } else {
        (normal_box, false)
    };

    let end_position = Vec3::new(
        (resolved_box.min.x + resolved_box.max.x) * 0.5,
        resolved_box.min.y,
        (resolved_box.min.z + resolved_box.max.z) * 0.5,
    );
    let resolved = end_position - position;
    Ok(ResolvedMotion {
        resolved,
        collisions: AxisCollisions {
            x: (velocity.x - resolved.x).abs() >= COLLISION_EPSILON,
            y: (velocity.y - resolved.y).abs() >= COLLISION_EPSILON,
            z: (velocity.z - resolved.z).abs() >= COLLISION_EPSILON,
        },
        identity,
        stepped,
    })
}

fn bounded_collision_boxes(
    world: &impl CollisionWorld,
    query: Aabb,
) -> Result<crate::CollisionQuery<Vec<Aabb>>, WorldQueryError> {
    crate::world::validate_collision_query(query)?;
    world.collision_boxes(query)
}

pub(super) fn clip_sneak_edge(
    world: &impl CollisionWorld,
    position: Vec3,
    velocity: Vec3,
) -> Result<(Vec3, Option<WorldCollisionIdentity>), WorldQueryError> {
    const OFFSET: f64 = 0.05;
    const MAX_ITERATIONS: usize = 24;
    let full_player = Aabb::player_at(position);
    let player = Aabb::new(
        Vec3::new(
            full_player.min.x + 0.025,
            full_player.min.y,
            full_player.min.z + 0.025,
        ),
        Vec3::new(
            full_player.max.x - 0.025,
            full_player.max.y,
            full_player.max.z - 0.025,
        ),
    );
    let mut clipped = velocity;
    let mut identity: Option<WorldCollisionIdentity> = None;
    for axis in [0, 2] {
        for _ in 0..MAX_ITERATIONS {
            if clipped[axis] == 0.0 {
                break;
            }
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
    for _ in 0..MAX_ITERATIONS {
        if clipped.x == 0.0 || clipped.z == 0.0 {
            break;
        }
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
        value - value.signum() * offset
    }
}

fn resolve_axes_reverse(start: Aabb, velocity: Vec3, colliders: &[Aabb]) -> (Aabb, Vec3) {
    let mut current = start;
    let mut resolved = Vec3::ZERO;
    for axis in [1, 0, 2] {
        let mut axis_velocity = Vec3::ZERO;
        axis_velocity[axis] = velocity[axis];
        for collider in colliders.iter().rev().copied() {
            axis_velocity = current.clip_against(collider, axis_velocity);
        }
        current = current.translated(axis_velocity);
        resolved += axis_velocity;
    }
    (current, resolved)
}

fn resolve_step(start: Aabb, velocity: Vec3, colliders: &[Aabb]) -> (Aabb, Vec3) {
    let mut current = start;
    let mut up = Vec3::new(0.0, STEP_HEIGHT, 0.0);
    for collider in colliders.iter().copied() {
        up = current.clip_against(collider, up);
    }
    current = current.translated(up);

    let mut horizontal = Vec3::ZERO;
    for axis in [0, 2] {
        let mut axis_velocity = Vec3::ZERO;
        axis_velocity[axis] = velocity[axis];
        for collider in colliders.iter().copied() {
            axis_velocity = current.clip_against(collider, axis_velocity);
        }
        current = current.translated(axis_velocity);
        horizontal += axis_velocity;
    }

    let mut down = up * -1.0;
    for collider in colliders.iter().copied() {
        down = current.clip_against(collider, down);
    }
    current = current.translated(down);
    (current, horizontal + up + down)
}
