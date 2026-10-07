//! Retained UI preparation regression and timing fixture.
use super::*;
use bevy::ecs::system::RunSystemOnce;

/// Builds a large immutable HUD in the no-op renderer.
fn retained_world() -> World {
    let mut world = ordered_command_tests::binding_world();
    let input = UiRenderInput {
        revision: 1,
        viewport_size: [1920, 1080],
        safe_area: [0; 4],
        vertices: vec![
            UiRenderVertex {
                position: [1.0; 2],
                clip_z: 0.0,
                clip_w: 1.0,
                uv: [0.0; 2],
                color: [255; 4],
                style_flags: 0,
                alpha_cutoff: -1.0,
                model_light: 1.0,
                overlay_color: [0.0; 4],
            };
            60_000
        ]
        .into(),
        indices: vec![0; 90_000].into(),
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 1920, 1080),
            0,
            90_000,
            0,
        )]),
        textures: Arc::new(
            render_model::UiTextureCatalog::new(
                vec![render_model::UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    let mut scene = UiRenderScene::default();
    scene
        .publish(input, world.resource::<UiRenderStatsResource>())
        .unwrap();
    world.insert_resource(UiRenderSceneResource(scene));
    world.run_system_once(prepare_ui_resources).unwrap();
    world
}

#[test]
fn retained_publication_rejects_conflicting_identity_and_missing_buffers() {
    let mut world = retained_world();
    world.run_system_once(prepare_ui_resources).unwrap();
    assert_eq!(world.resource::<UiGpu>().accepted_revision, Some(1));
    world.resource_mut::<UiGpu>().vertex_buffer = None;
    world.run_system_once(prepare_ui_resources).unwrap();
    assert_eq!(world.resource::<UiGpu>().accepted_revision, None);

    let mut world = retained_world();
    let input = world
        .resource::<UiRenderSceneResource>()
        .input
        .clone()
        .unwrap();
    let mut conflict = (*input).clone();
    conflict.viewport_size = [640, 480];
    world.resource_mut::<UiRenderSceneResource>().input = Some(Arc::new(conflict));
    world.run_system_once(prepare_ui_resources).unwrap();
    assert_eq!(world.resource::<UiGpu>().accepted_revision, None);
}

#[test]
fn unchanged_prepared_ui_submits_no_uploads() {
    let mut world = retained_world();
    world.insert_resource(profile::UiProfile::with_baseline_replay(false));
    for _ in 0..32 {
        world.run_system_once(prepare_ui_resources).unwrap();
    }
    assert_eq!(
        world.resource::<profile::UiProfile>().submitted_uploads(),
        [[0; 3]; 2],
        "an unchanged publication must not write geometry, textures or viewport bytes"
    );
}

#[test]
fn diagnostic_baseline_reproduces_unchanged_viewport_uploads() {
    const FRAMES: u64 = 32;
    let mut world = retained_world();
    world.insert_resource(profile::UiProfile::with_baseline_replay(true));
    for _ in 0..FRAMES {
        world.run_system_once(prepare_ui_resources).unwrap();
    }
    assert_eq!(
        world.resource::<profile::UiProfile>().submitted_uploads(),
        [
            [0, 0, FRAMES],
            [0, 0, FRAMES * size_of::<UiViewportUniform>() as u64]
        ]
    );
}

/// Measures the actual preparation system with an unchanged publication.
#[test]
#[ignore = "release performance measurement"]
fn frame_cost_bench_retained_ui_preparation() {
    let mut world = retained_world();
    let system = world.register_system(prepare_ui_resources);
    let started = std::time::Instant::now();
    for _ in 0..2_000 {
        world.run_system(system).unwrap();
    }
    eprintln!(
        "RETAINED_UI_BENCH vertices=60000 indices=90000 frames=2000 ms={:.3}",
        started.elapsed().as_secs_f64() * 1000.0
    );
    assert_eq!(world.resource::<UiGpu>().accepted_revision, Some(1));
}
