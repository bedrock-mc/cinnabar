use std::collections::BTreeSet;

use crate::world::DEFAULT_SURFACE_FRICTION;
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
    /// This tick's sneak descent through scaffolding, which removes its support.
    pub descend_through: bool,
    /// Primary facts of every cell already read this tick, sorted by cell.
    pub primaries: Vec<([i32; 3], crate::BlockPhysicsFacts)>,
    /// Move multiplier of the berry-bush and powder-snow cells the body is inside.
    pub stuck: Option<[f32; 3]>,
    /// Response of the block under the friction probe, which selects soul sand acceleration.
    pub friction_surface: SurfaceResponse,
    /// Honey at the feet cell, or below integer-aligned feet, scales the jump.
    pub honey_jump: bool,
}

impl SampledEnvironment {
    /// Reads a primary block, reusing this tick's earlier reads before spending the
    /// shared budget. Returns the identity of a fresh read for the caller to merge.
    pub(super) fn primary(
        &mut self,
        world: &(impl CollisionWorld + ?Sized),
        block: [i32; 3],
    ) -> Result<(crate::BlockPhysicsFacts, Option<WorldCollisionIdentity>), WorldQueryError> {
        match self
            .primaries
            .binary_search_by(|(cell, _)| cell.cmp(&block))
        {
            Ok(index) => Ok((self.primaries[index].1, None)),
            Err(index) => {
                let sample = sample_primary(world, block, &mut self.block_samples)?;
                self.primaries.insert(index, (block, sample.value));
                Ok((sample.value, Some(sample.identity)))
            }
        }
    }
}

/// Reads one primary block without exceeding the tick's shared physics budget.
pub(super) fn sample_primary(
    world: &(impl CollisionWorld + ?Sized),
    block: [i32; 3],
    block_samples: &mut usize,
) -> Result<crate::CollisionQuery<crate::BlockPhysicsFacts>, WorldQueryError> {
    if *block_samples == MAX_BLOCK_SAMPLES_PER_TICK {
        return Err(WorldQueryError::QueryExtentExceeded);
    }
    let sample = world.block_physics(block)?;
    *block_samples += 1;
    Ok(crate::CollisionQuery {
        value: *sample.primary(),
        identity: sample.identity,
    })
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
    let feet = block_at(position)?;
    let inside = inside_cells(player)?;
    let feet_aligned = position.y as f32 == (position.y as f32).floor();
    let friction_block = block_at(Vec3::new(
        f64::from(position.x as f32),
        f64::from(position.y as f32 - 0.1_f32),
        f64::from(position.z as f32),
    ))?;
    let mut blocks = BTreeSet::from([support, friction_block, feet]);
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
    let mut primaries = Vec::with_capacity(block_samples);
    let mut identity: Option<WorldCollisionIdentity> = None;
    let mut movement = MovementEnvironment::default();
    let mut friction = DEFAULT_SURFACE_FRICTION;
    let mut stuck = None;
    let mut friction_surface = SurfaceResponse::None;
    let (mut feet_honey, mut support_honey) = (false, false);
    for block in blocks {
        let sample = world.block_physics(block)?;
        primaries.push((block, *sample.primary()));
        identity = Some(match identity {
            None => sample.identity.clone(),
            Some(previous) => previous.merge(&sample.identity)?,
        });
        if block == friction_block && !probes_air(world, block, &mut identity)? {
            friction = sample.primary().friction;
            friction_surface = sample.primary().surface_response;
        }
        // Climbing reads only the block at the feet cell, never body contact.
        if block == feet {
            movement.on_climbable = sample
                .primary()
                .flags
                .contains(BlockPhysicsFlags::CLIMBABLE);
        }
        let honey = sample.primary().surface_response == SurfaceResponse::Honey;
        feet_honey |= block == feet && honey;
        support_honey |= block == support && honey;
        if block == support {
            let response = active_surface_response(sample.primary(), player, block);
            if response != SurfaceResponse::None {
                movement.surface_response = response;
            }
        }
        for facts in &sample.layers {
            let body_contact = fluid_intersects(player, block, 1.0);
            let active_response = if body_contact
                && matches!(
                    facts.surface_response,
                    SurfaceResponse::Honey
                        | SurfaceResponse::BubbleUp
                        | SurfaceResponse::BubbleDown
                ) {
                active_surface_response(facts, player, block)
            } else {
                SurfaceResponse::None
            };
            if movement.surface_response == SurfaceResponse::None
                && active_response != SurfaceResponse::None
            {
                movement.surface_response = active_response;
            }
            // Stuck-block slowdown belongs to the displacement phase, not acceleration.
            let stuck_block = facts.flags.contains(BlockPhysicsFlags::COBWEB)
                || facts.flags.contains(BlockPhysicsFlags::POWDER_SNOW)
                || is_slowdown_plant(facts);
            if stuck_block
                && !facts.flags.contains(BlockPhysicsFlags::COBWEB)
                && contains(inside, block)
            {
                stuck = Some(merge_stuck(
                    stuck,
                    [
                        facts.horizontal_speed_factor as f32,
                        facts.vertical_speed_factor as f32,
                        facts.horizontal_speed_factor as f32,
                    ],
                ));
            }
            if !stuck_block && (body_contact || (block == support && facts.flags.bits() == 0)) {
                movement.horizontal_speed_factor = movement
                    .horizontal_speed_factor
                    .min(facts.horizontal_speed_factor);
                movement.vertical_speed_factor = movement
                    .vertical_speed_factor
                    .min(facts.vertical_speed_factor);
            }
            movement.in_water |= facts.flags.contains(BlockPhysicsFlags::WATER)
                && liquid_contact(liquid_player, block, true);
            movement.in_lava |= facts.flags.contains(BlockPhysicsFlags::LAVA)
                && liquid_contact(liquid_player, block, false);
            // Cobwebs occupy a full block volume even without solid collision
            // boxes. Swept/support samples alone do not establish body contact.
            movement.in_cobweb |= facts.flags.contains(BlockPhysicsFlags::COBWEB)
                && fluid_intersects(player, block, 1.0);
            movement.in_powder_snow |=
                contains(inside, block) && facts.flags.contains(BlockPhysicsFlags::POWDER_SNOW);
        }
    }
    let identity = identity.expect("the support block guarantees one bounded sample");
    Ok(SampledEnvironment {
        movement,
        friction,
        identity,
        block_samples,
        descend_through: false,
        primaries,
        stuck,
        friction_surface,
        // Integer-aligned feet also read the block below the feet cell.
        honey_jump: feet_honey || (feet_aligned && support_honey),
    })
}

/// Vanilla keeps the default friction over air, such as with the feet probe past a block edge.
fn probes_air(
    world: &(impl CollisionWorld + ?Sized),
    block: [i32; 3],
    identity: &mut Option<WorldCollisionIdentity>,
) -> Result<bool, WorldQueryError> {
    let Some(air) = world.primary_is_air(block)? else {
        return Ok(false);
    };
    *identity = Some(match identity.take() {
        None => air.identity,
        Some(previous) => previous.merge(&air.identity)?,
    });
    Ok(air.value)
}

/// Checks a bounded volume against the same block-physics authority used for
/// movement, returning every queried identity so an exit probe cannot treat an
/// unloaded or mismatched liquid region as clear.
pub(super) fn contains_liquid(
    world: &impl CollisionWorld,
    query: Aabb,
    samples: &mut usize,
) -> Result<(bool, WorldCollisionIdentity), WorldQueryError> {
    crate::world::validate_collision_query(query)?;
    let min = block_at(query.min)?;
    let max = inclusive_max_block_at(query.max)?;
    let mut identity: Option<WorldCollisionIdentity> = None;
    let mut contains = false;
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                if *samples == MAX_BLOCK_SAMPLES_PER_TICK {
                    return Err(WorldQueryError::QueryExtentExceeded);
                }
                *samples += 1;
                let block = [x, y, z];
                let sample = world.block_physics(block)?;
                identity = Some(match identity {
                    None => sample.identity.clone(),
                    Some(previous) => previous.merge(&sample.identity)?,
                });
                // The raised exit probe asks whether its sampled block cells
                // carry liquid, not whether the liquid surface reaches it.
                // Vanilla reads only the primary block's material here,
                // without secondary layers.
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

/// A collision-free block with reduced speed factors (berry bush class).
fn is_slowdown_plant(facts: &crate::BlockPhysicsFacts) -> bool {
    let special = BlockPhysicsFlags::WATER.bits()
        | BlockPhysicsFlags::LAVA.bits()
        | BlockPhysicsFlags::COBWEB.bits()
        | BlockPhysicsFlags::POWDER_SNOW.bits()
        | BlockPhysicsFlags::SCAFFOLDING.bits();
    facts.flags.contains(BlockPhysicsFlags::PASSABLE)
        && facts.flags.bits() & special == 0
        && (facts.horizontal_speed_factor < 1.0 || facts.vertical_speed_factor < 1.0)
}

/// The first stuck block sets the multiplier; later ones keep the per-axis minimum.
pub(super) fn merge_stuck(current: Option<[f32; 3]>, next: [f32; 3]) -> [f32; 3] {
    match current {
        Some(current) if current.iter().any(|axis| axis.abs() >= f32::EPSILON) => {
            [0, 1, 2].map(|axis| current[axis].min(next[axis]))
        }
        _ => next,
    }
}

/// Cells the body occupies for block-inside effects: the box shrunk by 0.001 on every side.
pub(super) fn inside_cells(player: Aabb) -> Result<([i32; 3], [i32; 3]), WorldQueryError> {
    const SHRINK: f32 = 0.001;
    let low = block_at(Vec3::new(
        f64::from(player.min.x as f32 + SHRINK),
        f64::from(player.min.y as f32 + SHRINK),
        f64::from(player.min.z as f32 + SHRINK),
    ))?;
    let high = block_at(Vec3::new(
        f64::from(player.max.x as f32 - SHRINK),
        f64::from(player.max.y as f32 - SHRINK),
        f64::from(player.max.z as f32 - SHRINK),
    ))?;
    Ok((low, high))
}

pub(super) fn contains((low, high): ([i32; 3], [i32; 3]), block: [i32; 3]) -> bool {
    (0..3).all(|axis| low[axis] <= block[axis] && block[axis] <= high[axis])
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
