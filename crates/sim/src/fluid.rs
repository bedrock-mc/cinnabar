use crate::{Aabb, BlockPhysicsFlags, CollisionWorld, MAX_BLOCK_SAMPLES_PER_TICK, WorldQueryError};

/// Samples an actor's native water/lava contact probes from its authoritative body box.
/// Material cells establish contact independently of their rendered liquid surface height.
/// Missing world data is an error, so callers can retain the last known animation state.
pub fn sample_actor_liquids(
    world: &impl CollisionWorld,
    body: Aabb,
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
                let sample = world.block_physics(block)?;
                for layer in &sample.layers {
                    water |= water_contact && layer.flags.contains(BlockPhysicsFlags::WATER);
                    lava |= lava_contact && layer.flags.contains(BlockPhysicsFlags::LAVA);
                }
            }
        }
    }
    // Native liquid fetch selects lava material when both probes find contact.
    Ok((water && !lava, lava))
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
