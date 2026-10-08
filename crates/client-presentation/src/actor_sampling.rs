use chunk_pipeline::WorldStream;
use sim::{Aabb, Vec3, sample_actor_liquids, sample_liquid_submersion};

use crate::observations::CollisionLookup;

/// Samples the world state actor presentation reads: body liquid contact, breathing-point
/// submersion and the bed orientation under each sleeper.
pub fn sample_actor_world_state(stream: &mut WorldStream, collisions: &dyn CollisionLookup) {
    let mode = stream.network_id_mode();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
    );
    let probes = stream.authority().actor_fluid_probes();
    let vector = |[x, y, z]: [f32; 3]| Vec3::new(f64::from(x), f64::from(y), f64::from(z));
    let fluids: Vec<_> = probes
        .iter()
        .filter_map(|probe| {
            let bounds = Aabb::new(vector(probe.min), vector(probe.max));
            // Keep the previous sample when a probe crosses unavailable chunk data.
            let (water, lava) = sample_actor_liquids(&world, bounds).ok()?;
            Some((probe.runtime_id, water, lava))
        })
        .collect();
    let breathing: Vec<_> = probes
        .iter()
        .filter_map(|probe| {
            let submerged =
                sample_liquid_submersion(&world, vector(breathing_point(probe))).ok()?;
            Some((probe.runtime_id, submerged.value))
        })
        .collect();
    let beds: Vec<_> = stream
        .authority()
        .actor_bed_sample_points()
        .into_iter()
        .filter_map(|(runtime_id, block)| {
            let runtime = world.primary_runtime_id(block).ok()?;
            let state = collisions.block_canonical_state(mode, runtime)?;
            Some((runtime_id, bed_rotation_degrees(state)?))
        })
        .collect();
    stream.set_actor_fluids(&fluids);
    stream.set_actor_breathing_liquids(&breathing);
    stream.set_actor_bed_rotations(&beds);
}

/// The body-box point tested for submersion: the eye at nine tenths of the height, as vanilla
/// places a mob's eyes by default; per-entity breathing offsets are not modelled.
fn breathing_point(probe: &client_world::ActorFluidProbe) -> [f32; 3] {
    [
        (probe.min[0] + probe.max[0]) * 0.5,
        probe.min[1] + (probe.max[1] - probe.min[1]) * 0.9,
        (probe.min[2] + probe.max[2]) * 0.5,
    ]
}

/// Quarter turns of the bed's `direction` state as degrees; the origin needs native measurement.
fn bed_rotation_degrees(canonical_state: &str) -> Option<f32> {
    let serde_json::Value::Object(map) = serde_json::from_str(canonical_state).ok()? else {
        return None;
    };
    let value = map.get("direction")?;
    let direction = value.get("value").unwrap_or(value).as_i64()?;
    Some(direction.rem_euclid(4) as f32 * 90.0)
}

#[cfg(test)]
mod tests {
    use super::bed_rotation_degrees;

    #[test]
    fn bed_direction_reads_typed_and_plain_states() {
        assert_eq!(
            bed_rotation_degrees(r#"{"direction":{"type":"int","value":3}}"#),
            Some(270.0)
        );
        assert_eq!(bed_rotation_degrees(r#"{"direction":1}"#), Some(90.0));
        assert_eq!(bed_rotation_degrees(r#"{"head_piece_bit":true}"#), None);
    }
}
