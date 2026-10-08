//! Native level-event 2010's portal trail between the old and new dragon egg positions.

use crate::{SpawnRequest, molang::Rng};

const PARTICLES: u64 = 128;

/// Emits the native burst along the decoded block displacement, with independent cell jitter
/// and the direction variables consumed by the pack's parametric portal particle.
pub fn dragon_egg_teleport_requests(
    origin: [f32; 3],
    displacement: [i32; 3],
    seed: u64,
) -> impl Iterator<Item = SpawnRequest> {
    let mut rng = Rng::new(seed);
    let count = if origin.iter().all(|value| value.is_finite()) {
        PARTICLES
    } else {
        0
    };
    let destination: [f32; 3] =
        std::array::from_fn(|axis| (origin[axis] + displacement[axis] as f32).floor());
    (0..count).map(move |index| {
        let fraction = rng.unit();
        let direction: [f32; 3] = std::array::from_fn(|_| rng.range(-0.1, 0.1));
        let position = std::array::from_fn(|axis| {
            destination[axis]
                + (origin[axis] - destination[axis]) * fraction
                + rng.range(-0.5, 0.5)
                + if axis == 1 { 0.0 } else { 0.5 }
        });
        SpawnRequest {
            effect: "minecraft:portal_directional".into(),
            position,
            variables: ["direction.x", "direction.y", "direction.z"]
                .into_iter()
                .zip(direction)
                .map(|(name, value)| (name.into(), value))
                .collect(),
            seed: seed.wrapping_add(index),
            ..SpawnRequest::default()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LevelParticle, classify_level_event};

    #[test]
    fn dragon_egg_event_decodes_each_axis_sign_and_spawns_the_whole_trail() {
        let data = (5 << 16) | (3 << 8) | 7 | (1 << 24) | (1 << 26);
        let Some(LevelParticle::DragonEggTeleport { displacement }) =
            classify_level_event(2010, data)
        else {
            panic!("dragon egg teleport must be admitted as a particle event");
        };
        assert_eq!(displacement, [5, -3, 7]);
        assert!(crate::is_particle_level_event(2010));
        let requests: Vec<_> =
            dragon_egg_teleport_requests([10.0, 20.0, 30.0], displacement, 3).collect();
        assert_eq!(requests.len() as u64, PARTICLES);
        for request in &requests {
            assert_eq!(request.effect, "minecraft:portal_directional");
            assert!((10.0..=16.0).contains(&request.position[0]));
            assert!((16.5..=20.5).contains(&request.position[1]));
            assert!((30.0..=38.0).contains(&request.position[2]));
            assert_eq!(request.variables.len(), 3);
            assert!(
                request
                    .variables
                    .iter()
                    .all(|(_, value)| (-0.1..=0.1).contains(value))
            );
        }
        assert!(requests.iter().any(|request| request.position[0] < 11.0));
        assert!(requests.iter().any(|request| request.position[0] > 14.0));
        assert_eq!(
            classify_level_event(2010, 0),
            Some(LevelParticle::DragonEggTeleport {
                displacement: [0; 3]
            })
        );
        assert_eq!(
            dragon_egg_teleport_requests([f32::NAN; 3], [0; 3], 1).count(),
            0
        );
        // The native handler floors the destination even for fractional packet origins.
        assert!(
            dragon_egg_teleport_requests([10.9; 3], [0; 3], 3)
                .any(|request| request.position[0] < 10.8)
        );
    }

    #[test]
    fn dragon_egg_trail_reaches_the_particle_system_and_retires() {
        use crate::{EmptyWorld, ParticleSystem};

        let mut system = ParticleSystem::default();
        // Exercise the pack's one-particle, direction-variable parametric contract.
        let effect = serde_json::json!({"particle_effect": {
            "description": {"identifier": "minecraft:portal_directional",
                "basic_render_parameters": {
                    "material": "particles_alpha", "texture": "synthetic_portal"}},
            "components": {
                "minecraft:emitter_lifetime_once": {"active_time": 0.01},
                "minecraft:emitter_rate_instant": {"num_particles": 1},
                "minecraft:particle_lifetime_expression": {"max_lifetime": 0.2},
                "minecraft:particle_motion_parametric": {"relative_position": [
                    "v.direction.x * v.particle_age",
                    "v.direction.y * v.particle_age",
                    "v.direction.z * v.particle_age"]},
                "minecraft:particle_appearance_billboard": {"size": [0.1, 0.1]}
            }
        }});
        assert!(system.register_effect(&serde_json::to_vec(&effect).unwrap()));
        for request in dragon_egg_teleport_requests([0.0; 3], [8, -2, 4], 3) {
            assert!(system.spawn(&request).is_some());
        }
        system.tick(0.01, &EmptyWorld);
        assert_eq!(system.live_particles() as u64, PARTICLES);
        for _ in 0..30 {
            system.tick(0.01, &EmptyWorld);
        }
        assert_eq!((system.emitter_count(), system.live_particles()), (0, 0));
        assert_eq!(system.dropped_spawns, 0);
    }
}
