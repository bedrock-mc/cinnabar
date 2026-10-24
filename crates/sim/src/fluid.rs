use crate::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, CollisionWorld, MAX_BLOCK_SAMPLES_PER_TICK, Vec3,
    WorldQueryError,
};

// Head-water sensing compares the eye against liquid level / 9, offset one level down.
const LIQUID_LEVEL_DIVISOR: f32 = 9.0;
const HEAD_SURFACE_OFFSET: f32 = -1.0 / LIQUID_LEVEL_DIVISOR;

/// Samples an actor's native water/lava contact probes from its authoritative body box.
/// Material cells establish contact independently of their rendered liquid surface height.
/// Missing world data is an error, so callers can retain the last known animation state.
pub fn sample_actor_liquids(
    world: &impl CollisionWorld,
    body: Aabb,
) -> Result<(bool, bool), WorldQueryError> {
    sample_liquid_contacts(body, |block| {
        let mut bits = 0;
        world.visit_block_materials(block, &mut |layer| bits |= layer.flags.bits())?;
        Ok(
            BlockPhysicsFlags::from_bits(bits & BlockPhysicsFlags::KNOWN_BITS)
                .expect("known physics flag mask stays valid"),
        )
    })
}

/// Samples both native contact probes using the caller's material cache or world reads.
pub(crate) fn sample_liquid_contacts(
    body: Aabb,
    mut materials: impl FnMut([i32; 3]) -> Result<BlockPhysicsFlags, WorldQueryError>,
) -> Result<(bool, bool), WorldQueryError> {
    crate::world::validate_collision_query(body)?;
    let min = [body.min.x, body.min.y, body.min.z].map(|value| value.floor() as i32);
    let max = [body.max.x, body.max.y, body.max.z].map(|value| value.floor() as i32);
    let (mut water, mut lava) = (false, false);
    let mut samples = 0;
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                let block = [x, y, z];
                let water_contact = liquid_contact(body, block, true);
                let lava_contact = liquid_contact(body, block, false);
                if !water_contact && !lava_contact {
                    continue;
                }
                if samples == MAX_BLOCK_SAMPLES_PER_TICK {
                    return Err(WorldQueryError::QueryExtentExceeded);
                }
                samples += 1;
                let flags = materials(block)?;
                water |= water_contact && flags.contains(BlockPhysicsFlags::WATER);
                lava |= lava_contact && flags.contains(BlockPhysicsFlags::LAVA);
            }
        }
    }
    // Native liquid fetch selects lava material when both probes find contact.
    Ok((water && !lava, lava))
}

/// Compares the point with this primary material's liquid surface using f32 arithmetic.
pub(crate) fn material_submersion(
    facts: BlockPhysicsFacts,
    point: Vec3,
    block: [i32; 3],
    liquids: &[BlockPhysicsFlags],
) -> bool {
    if !liquids.iter().any(|liquid| facts.flags.contains(*liquid)) {
        return false;
    }
    (point.y as f32) < liquid_surface_y(facts, block[1])
}

/// Height of a liquid block's surface: the block top lowered by its depth state, where a
/// falling (full-height) liquid counts as a source.
pub(crate) fn liquid_surface_y(facts: BlockPhysicsFacts, block_y: i32) -> f32 {
    let level = if facts.fluid_height_blocks >= 1.0 {
        1.0
    } else {
        (LIQUID_LEVEL_DIVISOR - (facts.fluid_height_blocks as f32 * LIQUID_LEVEL_DIVISOR).round())
            .clamp(1.0, 8.0)
    };
    (i64::from(block_y) + 1) as f32 - (level / LIQUID_LEVEL_DIVISOR + HEAD_SURFACE_OFFSET)
}

/// Native liquid fetch shrinks each probe and clamps it to the body's center.
/// Its cell test floors minima and includes maximum faces.
pub(crate) fn liquid_contact(body: Aabb, block: [i32; 3], water: bool) -> bool {
    let probe = crate::world::liquid_probe_bounds(body, water);
    (0..3).all(|axis| {
        let low = (probe.min[axis] as f32).floor();
        let high = probe.max[axis] as f32;
        let coordinate = block[axis] as f32;
        low <= coordinate && coordinate <= high
    })
}
