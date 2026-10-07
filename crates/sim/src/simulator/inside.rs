//! Block-inside effects: the stuck-block move slowdown, standing on slime or
//! honey, and the post-move honey and bubble-column velocity changes.

use crate::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, CollisionWorld, SurfaceResponse, Vec3,
    WorldCollisionIdentity, WorldQueryError,
};

use super::{
    MovementInput, MovementMode,
    environment::{self, SampledEnvironment},
};

const COBWEB: [f32; 3] = [0.25, 0.05, 0.25];
const WEAVING_COBWEB: [f32; 3] = [0.5, 0.25, 0.5];
/// Standing on slime or honey damps horizontal velocity below this vertical speed.
const STAND_ON_MAX_VERTICAL: f32 = 0.1;
const HONEY_HORIZONTAL: f32 = 0.4;
const HONEY_MIN_VERTICAL: f32 = -0.12;
/// Per-cell bubble-column impulse: (step, limit) at the surface, then inside the column.
const BUBBLE_UP: [(f32, f32); 2] = [(0.1, 1.8), (0.06, 0.7)];
const BUBBLE_DOWN: [(f32, f32); 2] = [(-0.03, -0.9), (-0.03, -0.3)];

/// This tick's move multiplier from the blocks the body is stuck in.
pub(super) fn stuck_multiplier(
    sampled: &SampledEnvironment,
    input: &MovementInput,
) -> Option<[f32; 3]> {
    // Only creative flight is immune.
    if input.mode == MovementMode::Flying && input.creative_flight {
        return None;
    }
    let cobweb = sampled.movement.in_cobweb.then_some(if input.effects.weaving {
        WEAVING_COBWEB
    } else {
        COBWEB
    });
    match cobweb {
        Some(cobweb) => Some(environment::merge_stuck(sampled.stuck, cobweb)),
        None => sampled.stuck,
    }
}

/// Scales the move request; the caller zeroes velocity once the move resolves.
pub(super) fn slow_request(velocity: &mut Vec3, multiplier: [f32; 3]) {
    velocity.x = f64::from(velocity.x as f32 * multiplier[0]);
    velocity.y = f64::from(velocity.y as f32 * multiplier[1]);
    velocity.z = f64::from(velocity.z as f32 * multiplier[2]);
}

/// Damps horizontal velocity on slime or honey after friction, unless sneaking.
pub(super) fn stand_on(velocity: &mut Vec3, surface: SurfaceResponse, sneaking: bool) {
    let vertical = velocity.y as f32;
    if sneaking
        || vertical >= STAND_ON_MAX_VERTICAL
        || !matches!(surface, SurfaceResponse::Slime | SurfaceResponse::Honey)
    {
        return;
    }
    let factor = vertical.abs() * 0.2 + 0.4;
    velocity.x = f64::from(velocity.x as f32 * factor);
    velocity.z = f64::from(velocity.z as f32 * factor);
}

/// Applies bubble-column then honey velocity changes for every cell the moved
/// body is inside, in x, y, z cell order, merging fresh reads into `identity`.
pub(super) fn after_move(
    world: &impl CollisionWorld,
    player: Aabb,
    velocity: &mut Vec3,
    input: &MovementInput,
    sampled: &mut SampledEnvironment,
    identity: &mut WorldCollisionIdentity,
) -> Result<(), WorldQueryError> {
    let (low, high) = environment::inside_cells(player)?;
    let was_in_water = sampled.movement.in_water;
    let mut honey_cells = 0;
    let mut vertical = velocity.y as f32;
    for x in low[0]..=high[0] {
        for y in low[1]..=high[1] {
            for z in low[2]..=high[2] {
                let cell = primary(world, sampled, identity, [x, y, z])?;
                let impulses = match cell.surface_response {
                    SurfaceResponse::Honey => {
                        honey_cells += 1;
                        continue;
                    }
                    SurfaceResponse::BubbleUp => BUBBLE_UP,
                    SurfaceResponse::BubbleDown => BUBBLE_DOWN,
                    _ => continue,
                };
                // Ability flight ignores bubble columns.
                if input.mode == MovementMode::Flying
                    || !cell.flags.contains(BlockPhysicsFlags::WATER)
                {
                    continue;
                }
                let above = is_air(world, sampled, identity, [x, y + 1, z])?;
                let (step, limit) = impulses[usize::from(!above)];
                let pushed = vertical + step;
                vertical = if step > 0.0 {
                    pushed.min(limit)
                } else {
                    pushed.max(limit)
                };
            }
        }
    }
    velocity.y = f64::from(vertical);
    if !was_in_water {
        for _ in 0..honey_cells {
            velocity.x = f64::from(velocity.x as f32 * HONEY_HORIZONTAL);
            velocity.y = f64::from((velocity.y as f32).max(HONEY_MIN_VERTICAL));
            velocity.z = f64::from(velocity.z as f32 * HONEY_HORIZONTAL);
        }
    }
    Ok(())
}

/// Reads a primary block through the tick cache, merging a fresh read's identity.
pub(super) fn primary(
    world: &impl CollisionWorld,
    sampled: &mut SampledEnvironment,
    identity: &mut WorldCollisionIdentity,
    block: [i32; 3],
) -> Result<BlockPhysicsFacts, WorldQueryError> {
    let (facts, fresh) = sampled.primary(world, block)?;
    if let Some(fresh) = fresh {
        *identity = identity.merge(&fresh)?;
    }
    Ok(facts)
}

/// Air material, falling back to the bare passable fact set without material identity.
fn is_air(
    world: &impl CollisionWorld,
    sampled: &mut SampledEnvironment,
    identity: &mut WorldCollisionIdentity,
    block: [i32; 3],
) -> Result<bool, WorldQueryError> {
    let facts = primary(world, sampled, identity, block)?;
    Ok(match world.primary_is_air(block)? {
        Some(air) => {
            *identity = identity.merge(&air.identity)?;
            air.value
        }
        None => facts.flags == BlockPhysicsFlags::PASSABLE,
    })
}
