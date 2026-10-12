use std::time::Duration;

use assets::native_material::NativeMaterialFacts;

use super::{
    material_allows_leaf,
    random::AmbientRandom,
    sampler::{MAX_SAMPLES, MIN_SAMPLES, NEAR_SAMPLES, Sampler},
};

#[test]
fn native_ambient_mt_matches_the_reference_stream_across_twist_boundaries() {
    // Current native state/temper contracts identify MT19937; independently
    // generated with libc++ std::mt19937 at the native fallback seed.
    let witnesses = [
        (0, 3_499_211_612),
        (1, 581_869_302),
        (2, 3_890_346_734),
        (3, 3_586_334_585),
        (623, 4_020_325_887),
        (624, 4_178_893_912),
        (1299, 3_163_125_263),
    ];
    let mut random = AmbientRandom::new(5489);
    for index in 0..=witnesses.last().unwrap().0 {
        let value = random.next();
        if let Some((_, expected)) = witnesses.iter().find(|(sample, _)| *sample == index) {
            assert_eq!(value, *expected, "native MT output {index}");
        }
    }
}

#[test]
fn native_ambient_budget_starts_at_one_hundred_and_adapts_within_hard_bounds() {
    let mut sampler = Sampler::default();
    assert_eq!(sampler.sample_count, MIN_SAMPLES);
    sampler.finish([0.0; 3], Duration::from_micros(125));
    assert_eq!(sampler.sample_count, MIN_SAMPLES * 2);
    sampler.finish([0.0; 3], Duration::from_micros(1));
    assert_eq!(sampler.sample_count, MAX_SAMPLES);
    sampler.finish([0.0; 3], Duration::from_secs(1));
    assert_eq!(sampler.sample_count, MIN_SAMPLES);
    sampler.finish([0.0; 3], Duration::ZERO);
    assert_eq!(sampler.sample_count, MIN_SAMPLES);
}

#[test]
fn moving_sample_center_is_front_biased_only_below_full_rate_and_below_teleport_limit() {
    let mut sampler = Sampler::default();
    let moving = sampler.plan([0.5, 0.0, 0.0], [1.0, 0.0, 0.0]);
    assert_eq!(moving.center, [16, 0, 0]);
    assert_eq!(moving.near_radius, 24);
    let backwards = sampler.plan([-0.5, 0.0, 0.0], [1.0, 0.0, 0.0]);
    assert_eq!(backwards.center, [-1, 0, 0]);
    let teleport = sampler.plan([88.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    assert_eq!(teleport.center, [88, 0, 0]);
    assert_eq!(teleport.near_radius, 16);
    sampler.sample_count = MAX_SAMPLES;
    let full_rate = sampler.plan([0.5, 0.0, 0.0], [1.0, 0.0, 0.0]);
    assert_eq!(full_rate.center, [0, 0, 0]);
    assert_eq!(full_rate.near_radius, 16);
    sampler.finish([0.5, 0.0, 0.0], Duration::from_secs(1));
    assert_eq!(
        sampler.plan([0.5, 0.0, 0.0], [1.0, 0.0, 0.0]).center,
        [0, 0, 0]
    );
}

#[test]
fn gaussian_samples_consume_native_z_y_x_order_and_split_near_far_loops() {
    let mut sampler = Sampler::default();
    sampler.sample_count = MAX_SAMPLES;
    let plan = sampler.plan([20.0, -40.0, 60.0], [0.0, 0.0, 1.0]);
    let mut random = AmbientRandom::new(5489);
    let mut reference = AmbientRandom::new(5489);
    for index in 0..MAX_SAMPLES {
        let radius = if index < NEAR_SAMPLES { 16 } else { 32 };
        let z = (reference.next() % radius) as i32 - (reference.next() % radius) as i32;
        let y = (reference.next() % radius) as i32 - (reference.next() % radius) as i32;
        let x = (reference.next() % radius) as i32 - (reference.next() % radius) as i32;
        assert_eq!(
            plan.sample(index, &mut random),
            Some([20 + x, -40 + y, 60 + z])
        );
    }
}

#[test]
fn leaf_admission_reads_material_facts_and_denies_unresolved_support() {
    assert!(material_allows_leaf(NativeMaterialFacts::NonSolidNonLiquid));
    assert!(material_allows_leaf(
        NativeMaterialFacts::ProvisionalNonSolidNonLiquid
    ));
    for material in [
        NativeMaterialFacts::Unresolved,
        NativeMaterialFacts::Solid,
        NativeMaterialFacts::Liquid,
        NativeMaterialFacts::SolidAndLiquid,
    ] {
        assert!(!material_allows_leaf(material));
    }
}

#[test]
fn admitted_flower_material_emits_a_particle_draw_instance() {
    let registry = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let flower = registry
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:azure_bluet")
        .unwrap();
    let source: serde_json::Value =
        serde_json::from_slice(assets::native_material::NATIVE_MATERIAL_SOURCE_BYTES).unwrap();
    let material: NativeMaterialFacts =
        serde_json::from_value(source["materials"][flower.name.as_ref()].clone()).unwrap();
    let allowed = material_allows_leaf(material);
    let effect = serde_json::json!({"particle_effect": {
        "description": {"identifier": super::LEAF_EFFECT, "basic_render_parameters": {
            "material": "particles_alpha", "texture": "fixture:original_leaf"}},
        "components": {
            "minecraft:emitter_rate_manual": {"max_particles": 1},
            "minecraft:emitter_lifetime_expression": {},
            "minecraft:emitter_shape_point": {"offset": [0, 0, 0]},
            "minecraft:particle_lifetime_expression": {"max_lifetime": 15},
            "minecraft:particle_appearance_billboard": {"size": [0.15, 0.1], "facing_camera_mode": "rotate_xyz"},
            "minecraft:particle_appearance_tinting": {
                "color": ["variable.color.r", "variable.color.g", "variable.color.b", 1]}
        }
    }});
    let mut system = crate::ParticleSystem::default();
    assert!(system.register_effect(&serde_json::to_vec(&effect).unwrap()));
    if allowed {
        assert!(
            system
                .spawn_biome_tinted(super::LEAF_EFFECT, [0, 0, 0], [0.3, 0.7, 0.2, 1.0])
                .is_some()
        );
    }
    let view = crate::ParticleView {
        position: [0.5, 0.5, 3.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    let draw = system.build_draw(&view, &crate::EmptyWorld);
    write_material_preview(&draw);
    assert!(
        allowed,
        "the admitted flower material allows foliage particles"
    );
    assert_eq!(draw.opaque.len(), 1);
    assert_eq!(draw.opaque[0].color, [0.3, 0.7, 0.2, 1.0]);
}

/// Saves production particle instances for the optional original-art CPU preview.
fn write_material_preview(draw: &crate::DrawLists) {
    let Some(folder) = std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let instances: Vec<_> = draw
        .opaque
        .iter()
        .chain(&draw.blend)
        .chain(&draw.add)
        .map(|instance| {
            serde_json::json!({"center": instance.center_light, "axis_x": instance.axis_x,
            "axis_y": instance.axis_y, "color": instance.color})
        })
        .collect();
    std::fs::write(
        folder.join("falling-leaf-preview.json"),
        serde_json::to_vec(&instances).unwrap(),
    )
    .unwrap();
}
