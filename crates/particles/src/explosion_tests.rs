use crate::{EmptyWorld, LevelParticle, ParticleSystem, classify_level_event, named_request};

fn system() -> ParticleSystem {
    let mut system = ParticleSystem::default();
    for (identifier, lifetime) in [
        (
            "minecraft:dragon_death_explosion_emitter",
            "minecraft:emitter_lifetime_once",
        ),
        (
            "minecraft:huge_explosion_emitter",
            "minecraft:emitter_lifetime_looping",
        ),
    ] {
        // The two runtime definitions have the same burst contract and different lifetimes.
        let definition = serde_json::json!({"particle_effect": {
            "description": {"identifier": identifier, "basic_render_parameters": {
                "material": "particles_alpha", "texture": "synthetic_explosion"}},
            "components": {
                lifetime: {"active_time": 0.4},
                "minecraft:emitter_rate_steady": {"spawn_rate": 120, "max_particles": 50},
                "minecraft:particle_lifetime_expression": {"max_lifetime": "math.random(0.3,0.5)"},
                "minecraft:particle_appearance_billboard": {"size": [1, 1]}
            }
        }});
        assert!(system.register_effect(&serde_json::to_vec(&definition).unwrap()));
    }
    system
}

#[test]
fn level_and_legacy_explosions_emit_then_fully_retire() {
    for (event, data) in [
        (2025, -1),
        (2025, 1),
        (2025, 2),
        (2025, 6),
        (crate::triggers::LEVEL_EVENT_PARTICLE_FLAG | 16, 0),
        (crate::triggers::LEVEL_EVENT_PARTICLE_FLAG | 17, 0),
    ] {
        let Some(LevelParticle::Named {
            effect,
            spell_color,
        }) = classify_level_event(event, data)
        else {
            panic!("explosion trigger {event}/{data} must select an effect");
        };
        let mut system = system();
        assert!(
            system
                .spawn(&named_request(effect, [0.0; 3], spell_color))
                .is_some(),
            "explosion trigger {event}/{data} must resolve its authored definition"
        );
        system.tick(0.05, &EmptyWorld);
        assert!(system.live_particles() > 0, "the explosion must be visible");
        for _ in 0..20 {
            system.tick(0.05, &EmptyWorld);
        }
        assert_eq!(
            (system.emitter_count(), system.live_particles()),
            (0, 0),
            "explosion trigger {event}/{data} must retire after the authored burst"
        );
    }
}

#[test]
fn scalar_block_explosion_events_do_not_fabricate_a_huge_cloud() {
    assert!(classify_level_event(2026, 0).is_none());
}

#[test]
fn explicit_named_looping_effects_keep_their_authored_lifetime() {
    let mut system = system();
    assert!(
        system
            .spawn(&named_request(
                "minecraft:huge_explosion_emitter",
                [0.0; 3],
                None,
            ))
            .is_some()
    );
    for _ in 0..20 {
        system.tick(0.05, &EmptyWorld);
    }
    assert_eq!(system.emitter_count(), 1);
    assert!(system.live_particles() > 0);
}
