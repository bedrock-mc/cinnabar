use std::collections::BTreeSet;

use crate::{
    Aabb, BlockPhysicsFlags, CollisionWorld, SurfaceResponse, Vec3, WorldCollisionIdentity,
    WorldQueryError,
};

use super::MovementEnvironment;
use crate::fluid::liquid_contact;

pub const MAX_BLOCK_SAMPLES_PER_TICK: usize = 64;

pub(super) struct SampledEnvironment {
    pub movement: MovementEnvironment,
    pub friction: f64,
    pub identity: WorldCollisionIdentity,
    pub block_samples: usize,
}

/// Samples the movement pose's sweep and the preceding liquid-sensing pose.
pub(super) fn sample(
    world: &(impl CollisionWorld + ?Sized),
    position: Vec3,
    velocity: Vec3,
    height: f64,
    liquid_contact_height: Option<f64>,
) -> Result<SampledEnvironment, WorldQueryError> {
    let player = Aabb::player_with_height_at(position, height);
    let liquid_player =
        Aabb::player_with_height_at(position, liquid_contact_height.unwrap_or(height));
    let swept = player.swept(velocity);
    let swept = Aabb::new(
        swept.min.component_min(liquid_player.min),
        swept.max.component_max(liquid_player.max),
    );
    crate::world::validate_collision_query(swept)?;
    let min = block_at(swept.min)?;
    let max = inclusive_max_block_at(swept.max)?;
    let support = block_below(position)?;
    let mut blocks = BTreeSet::from([support]);
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                if blocks.len() == MAX_BLOCK_SAMPLES_PER_TICK {
                    return Err(WorldQueryError::QueryExtentExceeded);
                }
                blocks.insert([x, y, z]);
            }
        }
    }

    let block_samples = blocks.len();
    let mut identity: Option<WorldCollisionIdentity> = None;
    let mut movement = MovementEnvironment::default();
    let mut friction = 0.6;
    for block in blocks {
        let sample = world.block_physics(block)?;
        identity = Some(match identity {
            None => sample.identity.clone(),
            Some(previous) => previous.merge(&sample.identity)?,
        });
        if block == support {
            friction = sample.primary().friction;
            let response = active_surface_response(sample.primary(), player, block);
            if response != SurfaceResponse::None {
                movement.surface_response = response;
            }
        }
        for facts in &sample.layers {
            let active_response = active_surface_response(facts, player, block);
            if movement.surface_response == SurfaceResponse::None
                && active_response != SurfaceResponse::None
            {
                movement.surface_response = active_response;
            }
            // Web slowdown belongs to the displacement phase, not ground acceleration.
            let body_contact = fluid_intersects(player, block, 1.0);
            if !facts.flags.contains(BlockPhysicsFlags::COBWEB)
                && (body_contact || (block == support && facts.flags.bits() == 0))
            {
                movement.horizontal_speed_factor = movement
                    .horizontal_speed_factor
                    .min(facts.horizontal_speed_factor);
                movement.vertical_speed_factor = movement
                    .vertical_speed_factor
                    .min(facts.vertical_speed_factor);
            }
            movement.on_climbable |=
                body_contact && facts.flags.contains(BlockPhysicsFlags::CLIMBABLE);
            movement.in_water |= facts.flags.contains(BlockPhysicsFlags::WATER)
                && liquid_contact(liquid_player, block, true);
            movement.in_lava |= facts.flags.contains(BlockPhysicsFlags::LAVA)
                && liquid_contact(liquid_player, block, false);
            // Cobwebs occupy a full block volume even without solid collision
            // boxes. Swept/support samples alone do not establish body contact.
            movement.in_cobweb |= facts.flags.contains(BlockPhysicsFlags::COBWEB)
                && fluid_intersects(player, block, 1.0);
            movement.in_powder_snow |= (body_contact
                && facts.flags.contains(BlockPhysicsFlags::POWDER_SNOW))
                || is_inside_slowdown(facts, player, block);
            movement.in_scaffolding |=
                body_contact && facts.flags.contains(BlockPhysicsFlags::SCAFFOLDING);
        }
    }
    let identity = identity.expect("the support block guarantees one bounded sample");
    Ok(SampledEnvironment {
        movement,
        friction,
        identity,
        block_samples,
    })
}

/// Checks a bounded volume against the same block-physics authority used for
/// movement, returning every queried identity so an exit probe cannot treat an
/// unloaded or mismatched liquid region as clear.
pub(super) fn contains_liquid(
    world: &impl CollisionWorld,
    query: Aabb,
    previous_samples: usize,
) -> Result<(bool, WorldCollisionIdentity), WorldQueryError> {
    crate::world::validate_collision_query(query)?;
    let min = block_at(query.min)?;
    let max = inclusive_max_block_at(query.max)?;
    let mut identity: Option<WorldCollisionIdentity> = None;
    let mut samples = previous_samples;
    let mut contains = false;
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                if samples == MAX_BLOCK_SAMPLES_PER_TICK {
                    return Err(WorldQueryError::QueryExtentExceeded);
                }
                samples += 1;
                let block = [x, y, z];
                let sample = world.block_physics(block)?;
                identity = Some(match identity {
                    None => sample.identity.clone(),
                    Some(previous) => previous.merge(&sample.identity)?,
                });
                // The raised exit probe asks whether its sampled block cells
                // carry liquid, not whether the liquid surface reaches it.
                // Current BlockSource::containsAnyLiquid (0x031a7a20)
                // reads getBlock's primary material, without secondary layers.
                let flags = sample.primary().flags;
                contains |= flags.contains(BlockPhysicsFlags::WATER)
                    || flags.contains(BlockPhysicsFlags::LAVA);
            }
        }
    }
    Ok((
        contains,
        identity.expect("a finite non-empty probe samples at least one block"),
    ))
}

/// A collision-free block with reduced speed factors slows a body that overlaps it (berry bush class).
fn is_inside_slowdown(facts: &crate::BlockPhysicsFacts, player: Aabb, block: [i32; 3]) -> bool {
    let special = BlockPhysicsFlags::WATER.bits()
        | BlockPhysicsFlags::LAVA.bits()
        | BlockPhysicsFlags::COBWEB.bits()
        | BlockPhysicsFlags::POWDER_SNOW.bits()
        | BlockPhysicsFlags::SCAFFOLDING.bits();
    facts.flags.contains(BlockPhysicsFlags::PASSABLE)
        && facts.flags.bits() & special == 0
        && (facts.horizontal_speed_factor < 1.0 || facts.vertical_speed_factor < 1.0)
        && fluid_intersects(player, block, 1.0)
}

fn active_surface_response(
    facts: &crate::BlockPhysicsFacts,
    player: Aabb,
    block: [i32; 3],
) -> SurfaceResponse {
    if matches!(
        facts.surface_response,
        SurfaceResponse::BubbleUp | SurfaceResponse::BubbleDown
    ) && !(facts.flags.contains(BlockPhysicsFlags::WATER)
        && fluid_intersects(player, block, facts.fluid_height_blocks))
    {
        SurfaceResponse::None
    } else {
        facts.surface_response
    }
}

/// Tests body contact with a block volume used by non-liquid effects.
fn fluid_intersects(player: Aabb, block: [i32; 3], height: f64) -> bool {
    height > 0.0
        && player.min.x < f64::from(block[0]) + 1.0
        && player.max.x > f64::from(block[0])
        && player.min.y < f64::from(block[1]) + height
        && player.max.y > f64::from(block[1])
        && player.min.z < f64::from(block[2]) + 1.0
        && player.max.z > f64::from(block[2])
}

pub(super) fn block_below(position: Vec3) -> Result<[i32; 3], WorldQueryError> {
    block_at(Vec3::new(position.x, position.y - 0.5, position.z))
}

pub(super) fn block_at(position: Vec3) -> Result<[i32; 3], WorldQueryError> {
    let values = [position.x.floor(), position.y.floor(), position.z.floor()];
    if values.into_iter().any(|value| {
        !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX)
    }) {
        return Err(WorldQueryError::CoordinateOutOfRange);
    }
    Ok(values.map(|value| value as i32))
}

/// Converts an exclusive AABB maximum to its final included block using
/// `ceil(max) - 1`, without subtracting an inexact floating-point epsilon.
fn inclusive_max_block_at(maximum: Vec3) -> Result<[i32; 3], WorldQueryError> {
    let mut blocks = [0; 3];
    for (index, value) in [maximum.x, maximum.y, maximum.z].into_iter().enumerate() {
        if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
            return Err(WorldQueryError::CoordinateOutOfRange);
        }
        let block = value.ceil() - 1.0;
        if block < f64::from(i32::MIN) || block > f64::from(i32::MAX) {
            return Err(WorldQueryError::CoordinateOutOfRange);
        }
        blocks[index] = block as i32;
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two liquid probes have different vertical and horizontal contact margins.
    #[test]
    fn liquid_contact_uses_native_shrink_and_clamps_low_poses_to_their_center() {
        let near_surface = Aabb::player_at(Vec3::new(0.5, 0.5995, 0.5));
        assert!(!liquid_contact(near_surface, [0, 0, 0], true));
        assert!(liquid_contact(near_surface, [0, 0, 0], false));
        let near_side = Aabb::player_at(Vec3::new(1.25, 0.0, 0.5));
        assert!(liquid_contact(near_side, [0, 0, 0], true));
        assert!(!liquid_contact(near_side, [0, 0, 0], false));
        let swimmer = Aabb::player_with_height_at(Vec3::new(0.5, 0.6, 0.5), 0.6);
        assert!(liquid_contact(swimmer, [0, 0, 0], true));
    }
}
