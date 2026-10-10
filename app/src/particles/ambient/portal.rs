//! Fixed-tick portal particle and ambient sound emission.

use chunk_pipeline::WorldStream;
use particles::{ParticleSystem, SpawnRequest};

use super::AmbientParticles;
use crate::{movement::PhysicsCollisionRegistries, particles::world_adapter::StreamParticleWorld};

const NORTH_SOUTH: &str = "minecraft:portal_north_south";
const EAST_WEST: &str = "minecraft:portal_east_west";
const PARTICLES_PER_CALLBACK: f32 = 40.0;

pub(super) fn effects_present(system: &ParticleSystem) -> bool {
    system.has_effect(NORTH_SOUTH) || system.has_effect(EAST_WEST)
}

impl AmbientParticles {
    pub(super) fn try_portal(
        &mut self,
        block: [i32; 3],
        stream: &WorldStream,
        world: &StreamParticleWorld<'_>,
        collisions: &PhysicsCollisionRegistries,
        system: &mut ParticleSystem,
    ) {
        let Some(runtime_id) = world.block_runtime_id(block) else {
            return;
        };
        let mode = stream.network_id_mode();
        if collisions.block_identifier(mode, runtime_id) != Some(assets::NETHER_PORTAL_IDENTIFIER) {
            return;
        }
        // Native consumes the portal ambient sound roll before selecting its
        // particle effect. Keep the random stream aligned with later samples.
        let _sound_roll = self.random.bounded(10);
        let axis = assets::pinned_block_presentation_states()
            .get(mode, runtime_id)
            .portal_axis
            .unwrap_or_default();
        let effect = effect_for_axis(axis);
        let accepted = system.spawn(&request(effect, block));
        if self.diagnostics.enabled {
            self.diagnostics.portal_blocks += 1;
            self.diagnostics.portal_requests += u64::from(accepted.is_some());
        }
    }
}

/// Selects the effect for a predecoded axis, retaining the unknown-axis route.
fn effect_for_axis(axis: assets::PortalAxis) -> &'static str {
    match axis {
        assets::PortalAxis::X => NORTH_SOUTH,
        assets::PortalAxis::Z | assets::PortalAxis::Unknown => EAST_WEST,
    }
}

/// Creates the centered burst without changing authored motion or art.
fn request(effect: &str, block: [i32; 3]) -> SpawnRequest {
    SpawnRequest {
        effect: effect.to_owned(),
        position: block.map(|component| component as f32 + 0.5),
        variables: vec![("num_particles".to_owned(), PARTICLES_PER_CALLBACK)],
        ..SpawnRequest::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admitted_portal_axis_lookup_allocates_nothing() {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let record = records
            .iter()
            .find(|record| record.name.as_ref() == assets::NETHER_PORTAL_IDENTIFIER)
            .unwrap();
        let states = assets::pinned_block_presentation_states();
        let before = crate::tests::alloc_count::thread_allocations();
        let old: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let old_allocations = crate::tests::alloc_count::thread_allocations() - before;
        let before = crate::tests::alloc_count::thread_allocations();
        let state = states.get(assets::NetworkIdMode::Sequential, record.sequential_id);
        let new_allocations = crate::tests::alloc_count::thread_allocations() - before;
        assert!(state.portal_axis.is_some());
        assert!(old.get("portal_axis").is_some());
        assert_eq!(new_allocations, 0);
        println!(
            "portal state allocations: parse={old_allocations}, admitted lookup={new_allocations}"
        );
    }

    #[test]
    fn portal_axis_routes_keep_the_cross_plane_effect_names() {
        assert_eq!(effect_for_axis(assets::PortalAxis::X), NORTH_SOUTH);
        assert_eq!(effect_for_axis(assets::PortalAxis::Z), EAST_WEST);
        assert_eq!(effect_for_axis(assets::PortalAxis::Unknown), EAST_WEST);
    }

    #[test]
    fn portal_callback_uses_a_centered_pack_burst_without_overriding_particle_art_or_motion() {
        let request = request(NORTH_SOUTH, [-7, 12, 5]);
        assert_eq!(request.position, [-6.5, 12.5, 5.5]);
        assert_eq!(
            request.variables,
            [("num_particles".to_owned(), PARTICLES_PER_CALLBACK)]
        );
        assert!(request.basis.is_none());
        assert!(request.inherit_velocity.is_none());
    }
}
