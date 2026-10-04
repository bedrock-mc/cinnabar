use std::time::Duration;

use super::{
    AmbientParticles, material_allows_leaf,
    random::AmbientRandom,
    sampler::{MAX_SAMPLES, MIN_SAMPLES, NEAR_SAMPLES, Sampler},
};
use crate::movement::MAX_LOCAL_PHYSICS_TICKS_PER_FRAME;

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
use assets::BlockFlags;

#[test]
fn ambient_leaf_cadence_is_fixed_tick_not_render_frame_and_resets_without_a_backlog() {
    let mut ambient = AmbientParticles::default();
    let quarter_tick = world::TICK_DURATION / 4;
    for _ in 0..30 {
        for _ in 0..3 {
            assert_eq!(ambient.due_ticks(quarter_tick), 0);
        }
        assert_eq!(ambient.due_ticks(quarter_tick), 1);
    }
    assert_eq!(ambient.due_ticks(world::TICK_DURATION * 3), 3);
    assert_eq!(
        ambient.due_ticks(world::TICK_DURATION * 1000),
        MAX_LOCAL_PHYSICS_TICKS_PER_FRAME
    );
    assert_eq!(ambient.due_ticks(Duration::ZERO), 0);
    assert_eq!(ambient.due_ticks(quarter_tick), 0);
    ambient.reset();
    assert_eq!(ambient.due_ticks(world::TICK_DURATION - quarter_tick), 0);
    assert_eq!(ambient.due_ticks(quarter_tick), 1);
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
fn native_under_material_is_not_inferred_from_passability_or_cross_geometry() {
    assert!(material_allows_leaf(BlockFlags::AIR, Some("minecraft:air")));
    for name in [
        "short_grass",
        "fern",
        "poppy",
        "dandelion",
        "brown_mushroom",
        "red_mushroom",
    ] {
        assert!(material_allows_leaf(
            BlockFlags::empty(),
            Some(&format!("minecraft:{name}"))
        ));
    }
    for name in [
        "water",
        "flowing_water",
        "lava",
        "oak_leaves",
        "snow_layer",
        "stone",
        "custom:plant",
    ] {
        assert!(!material_allows_leaf(BlockFlags::empty(), Some(name)));
    }
    assert!(!material_allows_leaf(BlockFlags::empty(), None));
}
