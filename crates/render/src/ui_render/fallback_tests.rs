//! Recovery checks for screen UI and lazily compiled reconstruction pipelines.

use super::*;
use bevy::render::{Render, RenderSystems};

#[test]
fn native_views_do_not_compile_upscale_pipelines_during_loading() {
    let mut app = fixture(noop_renderer());
    let entity = view_target(app.sub_app_mut(RenderApp).world_mut(), [40, 30], [40, 30]);
    app.sub_app_mut(RenderApp)
        .world_mut()
        .entity_mut(entity)
        .remove::<crate::render_scale::RenderScaleOutput>();
    for _ in 0..3 {
        app.update();
    }
    let world = app.sub_app_mut(RenderApp).world_mut();
    let stages = world.resource_scope(|world, mut upscale: Mut<UpscalePipeline>| {
        upscale.stages(
            world.resource::<PipelineCache>(),
            render_scene::SCENE_COLOR_FORMAT,
        )
    });
    assert!(
        stages.ids().iter().all(|&id| world
            .resource::<PipelineCache>()
            .get_render_pipeline(id)
            .is_none()),
        "native loading must leave upscale variants uncompiled until scaling is requested"
    );
}

#[test]
fn bilinear_keeps_native_ui_available_while_either_fsr_stage_is_pending() {
    let Some(mut app) = raster_fixture() else {
        return;
    };
    let world = app.sub_app_mut(RenderApp).world_mut();
    let entity = view(world, [16, 16], [4, 4]);
    ready_pipelines(world);
    world.insert_resource(crate::render_scale::RenderScaleSharpen(Some(0.0)));
    let stages = world.get::<NativeUiTarget>(entity).unwrap().seed_pipelines;
    let pending = {
        let cache = world.resource::<PipelineCache>();
        cache.queue_render_pipeline(cache.get_render_pipeline_descriptor(stages.easu).clone())
    };
    for missing_easu in [false, true] {
        let mut native = world.get_mut::<NativeUiTarget>(entity).unwrap();
        native.seed_pipelines = stages;
        if missing_easu {
            native.seed_pipelines.easu = pending;
        } else {
            native.seed_pipelines.rcas = pending;
        }
        world.run_system_once(prepare_native_ui_targets).unwrap();
        let device = world.resource::<RenderDevice>().clone();
        let (_, commands) = crate::render_test_support::record(world, &device, |world, context| {
            let target = world.get::<ViewTarget>(entity).unwrap();
            let native = world.get::<NativeUiTarget>(entity).unwrap();
            assert!(
                native.seed(world, context, target),
                "bilinear must cover pending FSR"
            );
            assert!(native.present(world, context, target));
        });
        world.resource::<RenderQueue>().submit(commands);
        assert_eq!(
            world
                .get::<NativeUiTarget>(entity)
                .unwrap()
                .seed_bindings
                .lock()
                .unwrap()
                .len(),
            1
        );
    }
}

/// Reduces the extracted fixture camera before any view textures are allocated.
fn reduce_fixture_views(
    mut commands: Commands,
    mut views: Query<(Entity, &mut ExtractedCamera, &mut ExtractedView)>,
) {
    for (entity, mut camera, mut view) in &mut views {
        let output = camera.physical_target_size.unwrap();
        camera.physical_target_size = Some(output / 2);
        camera.physical_viewport_size = Some(output / 2);
        view.viewport = UVec4::new(0, 0, output.x / 2, output.y / 2);
        commands
            .entity(entity)
            .insert(crate::render_scale::RenderScaleOutput(output));
    }
}

/// Simulates all reconstruction stages being unavailable before each frame's preparation.
fn withhold_upscale_pipelines(mut views: Query<&mut NativeUiTarget>) {
    for mut native in &mut views {
        native.seed_pipelines = UpscalePipelines {
            bilinear: CachedRenderPipelineId::INVALID,
            easu: CachedRenderPipelineId::INVALID,
            rcas: CachedRenderPipelineId::INVALID,
        };
    }
}

#[test]
fn scaled_views_draw_screen_batches_without_native_allocation_or_usable_upscalers() {
    use crate::render_test_support::scene_raster::{
        SIDE, app_with_effects, frame, read_pixels, renderer,
    };
    use crate::ui_render::{UiRenderPlugin, composite::prepare_ui_layers};
    use render_model::{
        UI_BLEND_ALPHA, UiRenderBatch, UiRenderInput, UiRenderVertex, UiScissor, UiTextureCatalog,
        UiTexturePage,
    };
    use render_scene::{UiRenderSceneResource, UiRenderStatsResource};
    use std::sync::Arc;

    for allocation_failure in [true, false] {
        let Some(render) = renderer() else { return };
        let (mut app, _, image) = app_with_effects(render, |app| {
            app.add_plugins(UiRenderPlugin);
        });
        let render = app.sub_app_mut(RenderApp);
        let fixture = {
            use crate::render_test_support::scene_raster::{FixturePipelines, fixture_pipeline};
            let device = render.world().resource::<RenderDevice>();
            FixturePipelines {
                checker: fixture_pipeline(device, 1, "checker"),
                sloped_checker: fixture_pipeline(device, 1, "sloped_checker"),
                silhouette: fixture_pipeline(device, 1, "silhouette"),
                scene: 0,
            }
        };
        render.insert_resource(fixture);
        render.edit_schedule(Render, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        render.add_systems(
            Render,
            reduce_fixture_views
                .in_set(RenderSystems::CreateViews)
                .in_set(crate::render_scale::RenderScaleSystems),
        );
        if !allocation_failure {
            render.add_systems(
                Render,
                withhold_upscale_pipelines
                    .in_set(RenderSystems::PrepareResources)
                    .before(prepare_native_ui_targets)
                    .before(prepare_ui_layers),
            );
        }
        let _failure = crate::gpu_allocation::inject(move |label, _| {
            allocation_failure && label == "native-resolution UI scene"
        });
        // Let pipeline compilation settle before publishing the screen-space witness.
        for _ in 0..8 {
            frame(&mut app);
        }
        let input = UiRenderInput {
            revision: 1,
            viewport_size: [SIDE; 2],
            safe_area: [0; 4],
            vertices: [[16.0, 16.0], [48.0, 16.0], [48.0, 48.0], [16.0, 48.0]]
                .map(|position| UiRenderVertex {
                    position,
                    clip_z: 0.0,
                    clip_w: 1.0,
                    uv: [0.5; 2],
                    color: [255; 4],
                    style_flags: 0,
                    alpha_cutoff: -1.0,
                    model_light: 1.0,
                    overlay_color: [0.0; 4],
                })
                .into(),
            indices: Arc::from([0, 1, 2, 0, 2, 3]),
            batches: Arc::from([UiRenderBatch::new(
                0,
                UiScissor::new(20, 20, 24, 24),
                0,
                6,
                UI_BLEND_ALPHA,
            )]),
            textures: Arc::new(
                UiTextureCatalog::new(
                    vec![UiTexturePage::owned([1, 1], vec![255; 4].into()).unwrap()],
                    1,
                )
                .unwrap(),
            ),
        };
        let stats = app.world().resource::<UiRenderStatsResource>().clone();
        app.world_mut()
            .resource_mut::<UiRenderSceneResource>()
            .publish(input, &stats)
            .unwrap();
        test_time::eventually("scaled fallback screen UI", || {
            frame(&mut app);
            let pixels = read_pixels(&app, &image);
            pixels[(32 * SIDE as usize + 32) * 4..][..4] == [255; 4]
        });
        let pixels = read_pixels(&app, &image);
        assert_ne!(
            &pixels[(32 * SIDE as usize + 17) * 4..][..4],
            &[255; 4],
            "fallback clips in authored output coordinates"
        );
        let world = app.sub_app_mut(RenderApp).world_mut();
        assert!(
            world
                .query::<&NativeUiTarget>()
                .iter(world)
                .all(|native| !native.is_ready())
        );
        assert!(
            world
                .query::<&crate::ui_render::composite::UiLayerTexture>()
                .iter(world)
                .all(|layer| layer.presented_complete_content())
        );
    }
}
