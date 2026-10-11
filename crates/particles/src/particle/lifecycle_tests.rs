//! Authored fixtures exercise particle lifetime and motion contracts.
use super::*;
use crate::{
    atlas::ParticleAtlas,
    def::parse_effect,
    emitter::SpawnRequest,
    world::{BlockIdentity, EmptyWorld},
};

struct CellWorld(&'static str);
impl ParticleWorld for CellWorld {
    fn solid_boxes(&self, _: [f32; 3], _: [f32; 3], _: &mut Vec<[f32; 6]>) {}
    fn light(&self, _: [i32; 3]) -> (u8, u8) {
        (0, 15)
    }
    fn fluid(&self, _: [i32; 3]) -> Fluid {
        if self.0 == "minecraft:water" {
            Fluid::Water
        } else {
            Fluid::None
        }
    }
    fn block_identity(&self, _: [i32; 3]) -> Option<BlockIdentity<'_>> {
        Some(BlockIdentity(self.0))
    }
}

/// Creates one particle from independent authored component JSON.
fn emitter(components: serde_json::Value) -> Emitter {
    let mut common = serde_json::json!({
        "minecraft:emitter_lifetime_once": {"active_time": 0.01},
        "minecraft:emitter_rate_instant": {"num_particles": 1},
        "minecraft:emitter_shape_point": {"direction": [1, 0, 0]},
        "minecraft:particle_lifetime_expression": {"max_lifetime": 10},
        "minecraft:particle_appearance_billboard": {"size": [0.1, 0.1]}
    });
    common
        .as_object_mut()
        .unwrap()
        .extend(components.as_object().unwrap().clone());
    let json = serde_json::json!({"particle_effect": {
        "description": {"identifier": "fixture:lifecycle", "basic_render_parameters": {"material": "particles_alpha", "texture": "fixture"}},
        "components": common
    }});
    let def = Arc::new(parse_effect(&serde_json::to_vec(&json).unwrap()).unwrap());
    let mut emitter = Emitter::new(
        1,
        def,
        ParticleAtlas::default().fallback(),
        &SpawnRequest::default(),
        1,
    );
    emitter.advance(0.001, &mut Outputs::default(), 100);
    assert_eq!(emitter.particles.len(), 1);
    emitter
}

/// Advances a fixture through the live particle update path.
fn update(emitter: &mut Emitter, dt: f32, world: &dyn ParticleWorld) {
    emitter.update_particles(dt, world, &mut Vec::new(), &mut Outputs::default());
}

#[test]
fn block_lists_match_exact_identity_in_both_directions() {
    for block in [
        "minecraft:stone",
        "minecraft:water",
        "custom:water_decor",
        "minecraft:air",
    ] {
        for (condition, expected) in [
            (
                "minecraft:particle_expire_if_in_blocks",
                block != "minecraft:stone",
            ),
            (
                "minecraft:particle_expire_if_not_in_blocks",
                block == "minecraft:stone",
            ),
        ] {
            let mut effect = emitter(serde_json::json!({condition: ["minecraft:stone"]}));
            update(&mut effect, 0.1, &CellWorld(block));
            assert_eq!(
                !effect.particles.is_empty(),
                expected,
                "{condition}, {block}"
            );
        }
    }
    let mut effect =
        emitter(serde_json::json!({"minecraft:particle_expire_if_in_blocks": ["minecraft:water"]}));
    update(&mut effect, 0.1, &CellWorld("custom:water_decor"));
    assert_eq!(effect.particles.len(), 1);
    update(&mut effect, 0.1, &CellWorld("minecraft:water"));
    assert!(effect.particles.is_empty());
}

#[test]
fn expiration_uses_updated_variables_without_one_second_cutoff() {
    let mut effect = emitter(serde_json::json!({
        "minecraft:particle_lifetime_expression": {"expiration_expression": "v.expire"},
        "minecraft:particle_initialization": {"per_update_expression": "v.expire = v.particle_age >= 2;"}
    }));
    update(&mut effect, 1.5, &EmptyWorld);
    assert_eq!(effect.particles.len(), 1);
    update(&mut effect, 0.5, &EmptyWorld);
    assert!(effect.particles.is_empty());
}

#[test]
fn maximum_age_expires_when_expression_is_false() {
    let mut effect = emitter(
        serde_json::json!({"minecraft:particle_lifetime_expression": {"max_lifetime": 0.5, "expiration_expression": 0}}),
    );
    update(&mut effect, 0.5, &EmptyWorld);
    assert!(effect.particles.is_empty());
}

#[test]
fn parametric_direction_preserves_speed_and_handles_initial_rest() {
    for (speed, expected) in [(2.0, [0.0, 2.0, 0.0]), (0.0, [0.0, 3.0, 0.0])] {
        let mut effect = emitter(serde_json::json!({
            "minecraft:particle_initial_speed": speed,
            "minecraft:particle_motion_parametric": {"relative_position": ["v.particle_age", 0, 0], "direction": [0, 3, 0], "rotation": 45}
        }));
        update(&mut effect, 0.25, &EmptyWorld);
        let particle = &effect.particles[0];
        assert_eq!(particle.pos, [0.25, 0.0, 0.0]);
        assert_eq!(particle.vel, expected);
        assert_eq!(particle.rotation, 45.0);
    }
}

#[test]
fn absent_direction_retains_velocity_and_zero_direction_is_safe() {
    for (motion, expected) in [
        (
            serde_json::json!({"relative_position": [0, 0, 0]}),
            [2.0, 0.0, 0.0],
        ),
        (serde_json::json!({"direction": [0, 0, 0]}), [0.0; 3]),
    ] {
        let mut effect = emitter(
            serde_json::json!({"minecraft:particle_initial_speed": 2, "minecraft:particle_motion_parametric": motion}),
        );
        update(&mut effect, 0.1, &EmptyWorld);
        assert_eq!(effect.particles[0].vel, expected);
    }
}

#[test]
fn authored_direction_is_evaluated_again_on_each_update() {
    let mut effect = emitter(serde_json::json!({
        "minecraft:particle_initial_speed": 2,
        "minecraft:particle_motion_parametric": {"direction": ["v.particle_age < 0.5", "v.particle_age >= 0.5", 0]}
    }));
    update(&mut effect, 0.25, &EmptyWorld);
    assert_eq!(effect.particles[0].vel, [2.0, 0.0, 0.0]);
    update(&mut effect, 0.25, &EmptyWorld);
    assert_eq!(effect.particles[0].vel, [0.0, 2.0, 0.0]);
}

#[test]
fn direction_only_motion_preserves_existing_position_and_rotation() {
    let mut effect = emitter(
        serde_json::json!({"minecraft:particle_motion_parametric": {"direction": [0, 1, 0]}}),
    );
    effect.particles[0].pos = [3.0, 4.0, 5.0];
    effect.particles[0].rotation = 30.0;
    update(&mut effect, 0.1, &EmptyWorld);
    assert_eq!(effect.particles[0].pos, [3.0, 4.0, 5.0]);
    assert_eq!(effect.particles[0].rotation, 30.0);
}

#[test]
fn negligible_direction_stops_a_moving_particle() {
    let mut effect = emitter(serde_json::json!({
        "minecraft:particle_initial_speed": 2,
        "minecraft:particle_motion_parametric": {"direction": [0, 0.00005, 0]}
    }));
    update(&mut effect, 0.1, &EmptyWorld);
    assert_eq!(effect.particles[0].vel, [0.0; 3]);
}

#[test]
fn only_speed_below_float_epsilon_uses_the_authored_vector_directly() {
    for (speed, expected) in [
        (f32::EPSILON / 2.0, 3.0),
        (f32::EPSILON * 2.0, f32::EPSILON * 2.0),
    ] {
        let mut effect = emitter(serde_json::json!({
            "minecraft:particle_initial_speed": speed,
            "minecraft:particle_motion_parametric": {"direction": [0, 3, 0]}
        }));
        update(&mut effect, 0.1, &EmptyWorld);
        assert_eq!(effect.particles[0].vel, [0.0, expected, 0.0]);
    }
}

#[test]
fn authored_empty_block_lists_do_not_expire_particles() {
    for (condition, survives) in [
        ("minecraft:particle_expire_if_in_blocks", true),
        ("minecraft:particle_expire_if_not_in_blocks", true),
    ] {
        let mut effect = emitter(serde_json::json!({condition: []}));
        update(&mut effect, 0.1, &CellWorld("minecraft:stone"));
        assert_eq!(!effect.particles.is_empty(), survives);
    }
}

#[test]
fn absent_lifetime_component_preserves_existing_admission() {
    let json = serde_json::json!({"particle_effect": {
        "description": {"identifier": "fixture:absent", "basic_render_parameters": {
            "material": "particles_alpha", "texture": "fixture"}},
        "components": {"minecraft:particle_appearance_billboard": {"size": [0.1, 0.1]}}
    }});
    assert!(parse_effect(&serde_json::to_vec(&json).unwrap()).is_none());
}
