//! Retained UI preparation regression and timing fixture.
use super::resources::prepare_ui_resources;
use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_model::UiScissor;

#[test]
fn inverted_crosshair_preserves_scene_alpha_for_transparent_texels() {
    let blend = pipeline::ui_invert_blend_state();
    assert_eq!(blend.alpha.src_factor, BlendFactor::Zero);
    assert_eq!(blend.alpha.dst_factor, BlendFactor::One);
    assert_eq!(blend.alpha.operation, BlendOperation::Add);
    // The native invert equation still changes colour; it must leave the
    // opaque canvas alpha intact even where the crosshair texture has no ink.
    assert_eq!(blend.color.src_factor, BlendFactor::OneMinusDst);
    assert_eq!(blend.color.dst_factor, BlendFactor::OneMinusSrc);
}

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

/// Replay diagnostics retain the same static viewport and geometry upload behavior.
#[test]
fn unchanged_prepared_ui_submits_no_uploads_in_either_replay_mode() {
    for baseline_replay in [false, true] {
        let mut world = retained_world();
        world.insert_resource(profile::UiProfile::with_baseline_replay(baseline_replay));
        for _ in 0..32 {
            world.run_system_once(prepare_ui_resources).unwrap();
        }
        assert_eq!(
            world.resource::<profile::UiProfile>().submitted_uploads(),
            [[0; 3]; 2],
            "unchanged UI must not upload in replay mode {baseline_replay}"
        );
    }
}

#[test]
fn retained_publication_plans_no_vertex_or_index_uploads() {
    let mut world = retained_world();
    let input = world
        .resource::<UiRenderSceneResource>()
        .input
        .as_ref()
        .unwrap()
        .clone();
    let mut gpu = world.resource_mut::<UiGpu>();
    let before = crate::alloc_count::thread_allocations();
    let plan = gpu.uploads.plan(&input, false, false);
    let allocations = crate::alloc_count::thread_allocations() - before;
    assert!(plan.vertices.is_empty());
    assert!(plan.indices.is_empty());
    assert_eq!(allocations, 0);
}

/// The actual preparation system leaves the static viewport buffer untouched after admission.
#[test]
fn retained_publication_does_not_upload_an_unused_viewport_clock() {
    let mut world = retained_world();
    let writes = world.resource::<UiGpu>().viewport_uploads.writes;
    let geometry_writes = world.resource::<UiGpu>().geometry_writes;
    for _ in 0..3 {
        world.run_system_once(prepare_ui_resources).unwrap();
    }
    assert_eq!(world.resource::<UiGpu>().viewport_uploads.writes, writes);
    assert_eq!(geometry_writes, [1, 1]);
    assert_eq!(world.resource::<UiGpu>().geometry_writes, geometry_writes);
}

/// Incoming glint selects its current phase immediately, then leaving it restores static retention.
#[test]
fn retained_publication_switches_glint_uniforms_on_the_incoming_frame() {
    let mut world = retained_world();
    let stats = world.resource::<UiRenderStatsResource>().clone();
    let mut input = (**world
        .resource::<UiRenderSceneResource>()
        .input
        .as_ref()
        .unwrap())
    .clone();
    let initial_writes = world.resource::<UiGpu>().viewport_uploads.writes;
    world.resource_mut::<UiGpu>().started =
        std::time::Instant::now() - std::time::Duration::from_secs(40);
    for (revision, enabled) in [(2, true), (3, false)] {
        input.revision = revision;
        let mut vertices = input.vertices.to_vec();
        vertices[0].style_flags = if enabled {
            render_model::UI_STYLE_GLINT
        } else {
            0
        };
        input.vertices = vertices.into();
        world
            .resource_mut::<UiRenderSceneResource>()
            .publish(input.clone(), &stats)
            .unwrap();
        world.run_system_once(prepare_ui_resources).unwrap();
        let gpu = world.resource::<UiGpu>();
        assert_eq!(gpu.animated, enabled);
        assert_eq!(
            gpu.viewport_uploads.writes,
            initial_writes + revision as usize - 1
        );
    }
    let writes = world.resource::<UiGpu>().viewport_uploads.writes;
    for _ in 0..3 {
        world.run_system_once(prepare_ui_resources).unwrap();
    }
    assert_eq!(world.resource::<UiGpu>().viewport_uploads.writes, writes);
}
