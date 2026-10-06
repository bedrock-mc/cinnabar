use std::sync::Arc;

use serde_json::json;

use super::{Emitter, Outputs, SpawnRequest};
use crate::{EmptyWorld, ParticleSystem, atlas::ParticleAtlas, def::parse_effect};

fn effect(lifetime: &str, properties: serde_json::Value) -> Vec<u8> {
    let mut components = json!({
        "minecraft:emitter_rate_steady": {"spawn_rate": 6, "max_particles": 30},
        "minecraft:emitter_shape_point": {},
        "minecraft:particle_lifetime_expression": {"max_lifetime": 2},
        "minecraft:particle_appearance_billboard": {
            "size": [0.1, 0.1],
            "facing_camera_mode": "lookat_xyz",
            "uv": {"uv": [0, 0], "uv_size": [1, 1]}
        }
    });
    components[lifetime] = properties;
    serde_json::to_vec(&json!({
        "particle_effect": {
            "description": {
                "identifier": "test:persistent",
                "basic_render_parameters": {"material": "particles_alpha", "texture": "x"}
            },
            "components": components
        }
    }))
    .unwrap()
}

#[test]
fn authored_emitter_lifetimes_remain_active_after_long_elapsed_time() {
    let lifetimes = [
        (
            "minecraft:emitter_lifetime_expression",
            json!({"activation_expression": 1, "expiration_expression": 0}),
        ),
        (
            "minecraft:emitter_lifetime_looping",
            json!({"active_time": 1, "sleep_time": 1}),
        ),
        (
            "minecraft:emitter_lifetime_once",
            json!({"active_time": 7200}),
        ),
    ];
    for (lifetime, properties) in lifetimes {
        let bytes = effect(lifetime, properties);
        let def = Arc::new(parse_effect(&bytes).unwrap());
        let mut emitter = Emitter::new(
            1,
            def,
            ParticleAtlas::default().fallback(),
            &SpawnRequest::default(),
            3,
        );
        emitter.age = 3600.0;
        emitter.advance(0.25, &mut Outputs::default(), 30);
        assert!(!emitter.done, "{lifetime} ended before its authored expiry");
        assert_eq!(emitter.particles.len(), 1, "{lifetime} stopped emitting");
    }
}

#[test]
fn persistent_actor_emitter_stays_bounded_and_finishes_on_actor_removal() {
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&effect(
        "minecraft:emitter_lifetime_expression",
        json!({"activation_expression": 1, "expiration_expression": 0}),
    )));
    for runtime_id in 0..=crate::system::MAX_EMITTERS as u64 {
        assert!(
            system
                .spawn(&SpawnRequest {
                    effect: "test:persistent".to_owned(),
                    bound: Some((runtime_id, [0.0; 3])),
                    ..SpawnRequest::default()
                })
                .is_some()
        );
    }
    assert_eq!(system.emitter_count(), crate::system::MAX_EMITTERS);
    for emitter in system.emitters_mut() {
        emitter.age = 3600.0;
    }
    for _ in 0..16 {
        system.tick(0.25, &EmptyWorld);
        assert!(system.live_particles() <= crate::MAX_LIVE_PARTICLES);
        assert!(
            system
                .emitters_mut()
                .iter()
                .all(|emitter| emitter.particles.len() <= 30)
        );
    }
    assert_eq!(system.emitter_count(), crate::system::MAX_EMITTERS);
    assert!(system.live_particles() > 0);
    system.update_bound_emitters(|_, _| None);
    assert!(system.bound_emitters().is_empty());
    assert!(system.live_particles() > 0);
    for _ in 0..9 {
        system.tick(0.25, &EmptyWorld);
    }
    assert_eq!(system.live_particles(), 0);
    assert_eq!(system.emitter_count(), 0);
}
