use chunk_pipeline::WorldStream;
use sim::{Aabb, Vec3, sample_actor_liquids, sample_liquid_submersion};

use crate::observations::CollisionLookup;

/// Samples body liquid contact and breathing-point submersion for actor animation ticks.
pub fn sample_actor_world_state(stream: &mut WorldStream, collisions: &dyn CollisionLookup) {
    let mode = stream.network_id_mode();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
        Default::default(),
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
    stream.set_actor_fluids(&fluids);
    stream.set_actor_breathing_liquids(&breathing);
}

/// Samples bed headings before frame presentation, including frames without an actor tick.
pub fn sample_actor_bed_rotations(stream: &mut WorldStream, collisions: &dyn CollisionLookup) {
    let points = stream.authority().actor_bed_sample_points();
    if points.is_empty() {
        return;
    }
    let mode = stream.network_id_mode();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
        Default::default(),
    );
    let beds: Vec<_> = points
        .into_iter()
        .filter_map(|(runtime_id, block)| {
            Some((
                runtime_id,
                sampled_bed_rotation(&world, collisions, mode, block)?,
            ))
        })
        .collect();
    stream.set_actor_bed_rotations(&beds);
}

/// Resolves the shared bed-direction state for camera and actor sampling.
fn sampled_bed_rotation(
    world: &sim::PaletteWorld<'_>,
    collisions: &dyn CollisionLookup,
    mode: assets::NetworkIdMode,
    bed: [i32; 3],
) -> Option<f32> {
    let runtime = world.primary_runtime_id(bed).ok()?;
    collisions.bed_rotation_degrees(mode, runtime)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Fails if an empty sleeper set tries to borrow or sample the world registry.
    struct NoBedQueries;

    impl CollisionLookup for NoBedQueries {
        /// No sleeper means no registry borrow is needed.
        fn registry(&self, _: assets::NetworkIdMode) -> &sim::CollisionRegistry {
            panic!("an empty sleeper set must not query the registry")
        }

        /// No sleeper means no heading lookup is needed.
        fn bed_rotation_degrees(&self, _: assets::NetworkIdMode, _: u32) -> Option<f32> {
            panic!("an empty sleeper set must not query bed facts")
        }

        /// Bed sampling does not need block names.
        fn block_identifier(&self, _: assets::NetworkIdMode, _: u32) -> Option<&str> {
            panic!("bed sampling must not query block names")
        }
    }

    #[test]
    fn no_sleepers_skip_registry_and_terrain_queries() {
        let mut stream = WorldStream::new(protocol::WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        });
        sample_actor_bed_rotations(&mut stream, &NoBedQueries);
    }
}
