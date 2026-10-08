use super::*;
use crate::system::MAX_EMITTERS;

const TEST_EFFECT: &str = "test:cached_foliage";

fn effect(max: usize, local_position: bool) -> Vec<u8> {
    let json = serde_json::json!({"particle_effect": {
    "description": {"identifier": TEST_EFFECT, "basic_render_parameters": {
        "material": "particles_alpha", "texture": "test"}},
    "components": {
        "minecraft:emitter_rate_manual": {"max_particles": max},
        "minecraft:emitter_lifetime_expression": {},
        "minecraft:emitter_local_space": {"position": local_position},
        "minecraft:emitter_shape_point": {"offset": [0, 0, 0]},
            "minecraft:particle_lifetime_expression": {"max_lifetime": 15},
            "minecraft:particle_appearance_billboard": {"size": [0.1, 0.1]},
        "minecraft:particle_appearance_tinting": {
            "color": ["variable.color.r", "variable.color.g", "variable.color.b", 1]}
    }}});
    serde_json::to_vec(&json).unwrap()
}

fn system(max: usize, local_position: bool) -> ParticleSystem {
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&effect(max, local_position)));
    system
}

#[test]
fn cached_foliage_keeps_one_emitter_and_independent_particle_origins() {
    let count = MAX_EMITTERS + 1;
    let mut system = system(count, false);
    let color = [0.2, 0.4, 0.6, 1.0];
    let mut first_id = None;
    for index in 0..count {
        let block = [(index % 32) as i32, 0, (index / 32) as i32];
        let id = system
            .spawn_biome_tinted(TEST_EFFECT, block, color)
            .unwrap();
        assert_eq!(id, *first_id.get_or_insert(id));
    }
    assert_eq!(system.emitter_count(), 1);
    assert_eq!(system.live_particles(), count);
    let emitter = &system.emitters[0];
    assert_eq!(emitter.world_position(0), [0.5; 3]);
    assert_eq!(emitter.world_position(1), [1.5, 0.5, 0.5]);
    assert_eq!(emitter.world_position(32), [0.5, 0.5, 1.5]);
}

#[test]
fn cached_foliage_honors_pack_manual_capacity_not_the_ordinary_burst_clamp() {
    let mut system = system(3, false);
    for index in 0..5 {
        system.spawn_biome_tinted(TEST_EFFECT, [index, 0, 0], [1.0; 4]);
    }
    assert_eq!(system.live_particles(), 3);
    assert_eq!(system.emitter_count(), 1);
}

#[test]
fn cached_foliage_reuses_an_empty_manual_emitter_after_its_particles_expire() {
    let mut system = system(3, false);
    let first = system.spawn_biome_tinted(TEST_EFFECT, [0; 3], [1.0; 4]);
    for _ in 0..64 {
        system.tick(0.25, &crate::world::EmptyWorld);
    }
    assert_eq!(system.live_particles(), 0);
    assert_eq!(system.emitter_count(), 1);
    assert_eq!(
        system.spawn_biome_tinted(TEST_EFFECT, [1, 0, 0], [1.0; 4]),
        first
    );
    assert_eq!(system.live_particles(), 1);
}

#[test]
fn cached_foliage_keys_rgba8_and_does_not_reuse_replaced_definitions() {
    let mut system = system(10, false);
    let first = system.spawn_biome_tinted(TEST_EFFECT, [0; 3], [0.5; 4]);
    let same = system.spawn_biome_tinted(TEST_EFFECT, [1, 0, 0], [0.5001; 4]);
    let other = system.spawn_biome_tinted(TEST_EFFECT, [2, 0, 0], [0.6; 4]);
    assert_eq!(same, first);
    assert_ne!(other, first);
    assert!(system.register_effect(&effect(10, false)));
    let replaced = system.spawn_biome_tinted(TEST_EFFECT, [0; 3], [0.5; 4]);
    assert_ne!(replaced, first);
    system.clear();
    assert_eq!(system.emitter_count(), 0);
}

#[test]
fn local_space_overrides_keep_individual_emitters_and_unusable_inputs_are_skipped() {
    let mut system = system(10, true);
    let first = system.spawn_biome_tinted(TEST_EFFECT, [0; 3], [1.0; 4]);
    let second = system.spawn_biome_tinted(TEST_EFFECT, [1, 0, 0], [1.0; 4]);
    assert_ne!(first, second);
    assert_eq!(system.emitter_count(), 2);
    assert!(
        system
            .spawn_biome_tinted(TEST_EFFECT, [0; 3], [f32::NAN; 4])
            .is_none()
    );
    assert!(
        system
            .spawn_biome_tinted(TEST_EFFECT, [i32::MAX; 3], [1.0; 4])
            .is_none()
    );
}

#[test]
fn emitter_relative_custom_motion_and_kill_planes_do_not_share_origins() {
    for (component, value) in [
        (
            "minecraft:particle_motion_parametric",
            serde_json::json!({"relative_position": [0, 0, 0]}),
        ),
        (
            "minecraft:particle_kill_plane",
            serde_json::json!([0, 1, 0, 0]),
        ),
    ] {
        let mut json: serde_json::Value = serde_json::from_slice(&effect(10, false)).unwrap();
        json["particle_effect"]["components"][component] = value;
        let mut system = ParticleSystem::default();
        assert!(system.register_effect(&serde_json::to_vec(&json).unwrap()));
        let first = system.spawn_biome_tinted(TEST_EFFECT, [0; 3], [1.0; 4]);
        let second = system.spawn_biome_tinted(TEST_EFFECT, [1, 0, 0], [1.0; 4]);
        assert_ne!(first, second, "{component}");
        assert_eq!(system.emitter_count(), 2);
    }
}

#[test]
#[ignore = "requires PINNED_BEDROCK_SAMPLES_PACK pointing at the pinned full sample resource_pack"]
fn cached_foliage_uses_actual_pinned_shape_motion_and_colour_variables() {
    let pack = std::path::PathBuf::from(std::env::var_os("PINNED_BEDROCK_SAMPLES_PACK").unwrap());
    let bytes = std::fs::read(pack.join("particles/biome_tinted_leaves_particle.json")).unwrap();
    let source: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let effect = &source["particle_effect"];
    let name = effect["description"]["identifier"].as_str().unwrap();
    let components = &effect["components"];
    let vector = |value: &serde_json::Value| {
        std::array::from_fn::<_, 3, _>(|axis| value[axis].as_f64().unwrap() as f32)
    };
    let shape = &components["minecraft:emitter_shape_box"];
    let offset = vector(&shape["offset"]);
    let half = vector(&shape["half_dimensions"]);
    let velocity = vector(&components["minecraft:particle_initial_speed"]);
    let lifetime = components["minecraft:particle_lifetime_expression"]["max_lifetime"]
        .as_f64()
        .unwrap() as f32;
    let color = [0.2, 0.4, 0.6, 1.0];
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&bytes));
    for x in 0..32 {
        assert!(system.spawn_biome_tinted(name, [x, 0, 0], color).is_some());
    }
    assert_eq!(system.emitter_count(), 1);
    let emitter = &system.emitters[0];
    assert_eq!(emitter.particles.len(), 32);
    assert!(emitter.def.particle.lit);
    for (x, particle) in emitter.particles.iter().enumerate() {
        // Native emission origin, followed by the unmodified pack shape.
        let center = [x as f32 + 0.5, 0.5, 0.5];
        for axis in 0..3 {
            let relative = particle.pos[axis] - center[axis];
            assert!(relative >= offset[axis] - half[axis] - f32::EPSILON);
            assert!(relative <= offset[axis] + half[axis] + f32::EPSILON);
        }
        assert_eq!(particle.vel, velocity);
        assert_eq!(particle.lifetime, lifetime);
        for (component, expected) in ["r", "g", "b"].into_iter().zip(color) {
            let slot = emitter
                .def
                .interner
                .get(&format!("color.{component}"))
                .unwrap();
            assert_eq!(particle.vars[usize::from(slot)], expected);
        }
    }
}
