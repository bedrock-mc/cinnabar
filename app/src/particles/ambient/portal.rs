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
        let state = collisions.block_canonical_state(mode, runtime_id);
        let effect = effect_for_state(state);
        let accepted = system.spawn(&request(effect, block));
        if self.diagnostics.enabled {
            self.diagnostics.portal_blocks += 1;
            self.diagnostics.portal_requests += u64::from(accepted.is_some());
        }
    }
}

fn effect_for_state(state: Option<&str>) -> &'static str {
    let axis_x = state
        .and_then(|state| serde_json::from_str::<serde_json::Value>(state).ok())
        .is_some_and(|state| state["portal_axis"]["value"].as_str() == Some("x"));
    // Vanilla portal axis x (1) selects the north/south effect. Unknown (0)
    // follows the same east/west route as Z (2), without inferring neighbors.
    if axis_x { NORTH_SOUTH } else { EAST_WEST }
}

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
    fn portal_axis_routes_match_the_native_cross_plane_effect_names() {
        assert_eq!(
            effect_for_state(Some(r#"{"portal_axis":{"type":"string","value":"x"}}"#)),
            NORTH_SOUTH
        );
        for axis in ["z", "unknown", "custom"] {
            let state = format!(r#"{{"portal_axis":{{"type":"string","value":"{axis}"}}}}"#);
            assert_eq!(effect_for_state(Some(&state)), EAST_WEST);
        }
        assert_eq!(effect_for_state(None), EAST_WEST);
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
