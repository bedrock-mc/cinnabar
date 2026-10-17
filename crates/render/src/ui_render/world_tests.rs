//! Native comparisons between a shared world-UI pass and the former depth-mode boundaries.

use crate::render_test_support::scene_raster::{
    FixturePipelines, SIDE, app_with_effects, capture, fixture_pipeline_format, frame, read_pixels,
    renderer,
};
use crate::ui_render::{
    UiGpu, UiRenderPlugin,
    overlay::{
        UiWorldLabel,
        world::{UiViewQuery, draw_ui_view},
    },
};
use bevy::{
    core_pipeline::{Core3d, Core3dSystems},
    prelude::{App, IntoScheduleConfigs, Msaa, Res, ResMut, Resource, World},
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::CachedRenderPipelineId,
        renderer::{RenderContext, RenderDevice},
    },
};
use render_model::{
    UI_BLEND_ALPHA, UI_BLEND_INVERT, UiRenderBatch, UiRenderInput, UiRenderVertex, UiScissor,
    UiTextureCatalog, UiTexturePage,
};
use render_scene::{UiRenderSceneResource, UiRenderStatsResource};
use std::sync::Arc;

#[derive(Resource, Default)]
struct Segmented {
    separate: bool,
    missing_attached: bool,
    missing_depth: bool,
}

/// Replays the former pass boundaries using the same pipelines, batches, and draw implementation.
fn draw_reference(
    world: &World,
    query: bevy::render::renderer::ViewQuery<UiViewQuery>,
    mut context: RenderContext,
) {
    let state = world.resource::<Segmented>();
    let Some(gpu) = world.get_resource::<UiGpu>() else {
        return;
    };
    let entity = query.entity();
    let mut view = query.into_inner();
    if state.missing_depth {
        view.4 = None;
    }
    if state.separate {
        let mut start = 0;
        while let Some(first) = gpu.batches.get(start) {
            let length = gpu.batches[start..]
                .iter()
                .take_while(|batch| {
                    (batch.depth_test, batch.depth_write) == (first.depth_test, first.depth_write)
                })
                .count();
            draw_ui_view(
                entity,
                &mut context,
                view,
                world,
                gpu,
                &gpu.batches[start..start + length],
            )
            .unwrap();
            start += length;
        }
    } else {
        draw_ui_view(entity, &mut context, view, world, gpu, &gpu.batches).unwrap();
    }
}

/// Simulates a compatible variant still compiling after ordinary material pipelines are ready.
fn block_compatible_pipelines(state: Res<Segmented>, mut gpu: ResMut<UiGpu>) {
    if state.missing_attached {
        for ((_, _, _, attached), pair) in &mut gpu.world_view_pipelines {
            if *attached {
                *pair = (
                    CachedRenderPipelineId::INVALID,
                    CachedRenderPipelineId::INVALID,
                );
            }
        }
    }
}

/// Publishes overlapping plates, ordinary text, environmental text, and an invert draw.
fn publish(app: &mut App, revision: u64, modes: &[(bool, bool)]) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut batches = Vec::new();
    for (index, &(test, write)) in modes.iter().enumerate() {
        let base = vertices.len() as u32;
        let first = indices.len() as u32;
        let left = 4.3 + index as f32 * 6.1;
        for position in [
            [left, 8.2],
            [left + 23.7, 8.2],
            [left + 23.7, 53.6],
            [left, 53.6],
        ] {
            vertices.push(UiRenderVertex {
                position,
                clip_z: if index % 2 == 0 { 0.7 } else { 0.3 },
                clip_w: 1.0,
                uv: [0.5; 2],
                color: [30 + index as u8 * 24, 180, 120, 170],
                style_flags: 0,
                alpha_cutoff: -1.0,
                model_light: 1.0,
                overlay_color: [0.0; 4],
            });
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        batches.push(
            UiRenderBatch::new(
                0,
                UiScissor::new(2, 3, SIDE - 4, SIDE - 6),
                first,
                6,
                if index == 3 {
                    UI_BLEND_INVERT
                } else {
                    UI_BLEND_ALPHA
                },
            )
            .with_world_projection(true)
            .with_depth_test(test)
            .with_depth_write(write),
        );
    }
    let input = UiRenderInput {
        revision,
        viewport_size: [SIDE; 2],
        safe_area: [0; 4],
        vertices: vertices.into(),
        indices: indices.into(),
        batches: batches.into(),
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
}

#[test]
fn shared_world_ui_preserves_pixels_across_depth_modes_and_sample_counts() {
    let Some(render) = renderer() else {
        return;
    };
    let (mut app, camera, image) = app_with_effects(render, |app| {
        app.add_plugins(UiRenderPlugin);
    });
    app.sub_app_mut(RenderApp)
        .init_resource::<Segmented>()
        .add_systems(
            Render,
            block_compatible_pipelines
                .after(RenderSystems::PrepareBindGroups)
                .before(RenderSystems::Render),
        );
    app.sub_app_mut(RenderApp)
        .world_mut()
        .schedule_scope(Core3d, |world, schedule| {
            schedule
                .remove_systems_in_set(
                    UiWorldLabel,
                    world,
                    bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
                )
                .unwrap();
            schedule.add_systems(
                draw_reference
                    .in_set(UiWorldLabel)
                    .after(crate::scene_target::ScenePass::Transparent)
                    .before(crate::scene_target::ScenePass::Finish)
                    .in_set(Core3dSystems::MainPass),
            );
        });
    let cases: &[&[(bool, bool)]] = &[
        &[
            (true, false),
            (false, false),
            (true, true),
            (false, false),
            (false, true),
            (true, false),
        ],
        &[
            (false, false),
            (false, false),
            (false, false),
            (false, false),
        ],
        &[(true, false), (false, false), (true, false), (false, false)],
    ];
    let mut revision = 0;
    for msaa in [Msaa::Off, Msaa::Sample4] {
        for hdr in [false, true] {
            *app.world_mut().get_mut::<Msaa>(camera).unwrap() = msaa;
            if hdr {
                app.world_mut().entity_mut(camera).insert(bevy::camera::Hdr);
            } else {
                app.world_mut()
                    .entity_mut(camera)
                    .remove::<bevy::camera::Hdr>();
            }
            let device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
            let format = if hdr {
                render_scene::SCENE_HDR_FORMAT
            } else {
                render_scene::SCENE_COLOR_FORMAT
            };
            let fixture = FixturePipelines {
                checker: fixture_pipeline_format(device, msaa.samples(), format, "checker"),
                sloped_checker: fixture_pipeline_format(
                    device,
                    msaa.samples(),
                    format,
                    "sloped_checker",
                ),
                silhouette: fixture_pipeline_format(device, msaa.samples(), format, "silhouette"),
                scene: 0,
            };
            app.sub_app_mut(RenderApp).insert_resource(fixture);
            for (missing_attached, missing_depth) in [(false, false), (true, false), (false, true)]
            {
                for modes in cases {
                    revision += 1;
                    publish(&mut app, revision, modes);
                    *app.sub_app_mut(RenderApp)
                        .world_mut()
                        .resource_mut::<Segmented>() = Segmented {
                        separate: true,
                        missing_attached,
                        missing_depth,
                    };
                    for _ in 0..8 {
                        frame(&mut app);
                    }
                    let expected = read_pixels(&app, &image);
                    let before_passes = app
                        .sub_app(RenderApp)
                        .world()
                        .resource::<UiGpu>()
                        .world_passes
                        .load(std::sync::atomic::Ordering::Relaxed);
                    app.sub_app_mut(RenderApp)
                        .world_mut()
                        .resource_mut::<Segmented>()
                        .separate = false;
                    frame(&mut app);
                    let actual = read_pixels(&app, &image);
                    let after_passes = app
                        .sub_app(RenderApp)
                        .world()
                        .resource::<UiGpu>()
                        .world_passes
                        .load(std::sync::atomic::Ordering::Relaxed);
                    let shared_passes = if missing_attached && !missing_depth {
                        1 + modes
                            .windows(2)
                            .filter(|pair| (pair[0].0 || pair[0].1) != (pair[1].0 || pair[1].1))
                            .count()
                    } else {
                        1
                    };
                    assert_eq!(after_passes, shared_passes);
                    let expected_passes = modes
                        .iter()
                        .enumerate()
                        .filter(|(i, mode)| {
                            (*i == 0 || **mode != modes[*i - 1])
                                && (!missing_depth || **mode == (false, false))
                        })
                        .count();
                    assert_eq!(before_passes, expected_passes);
                    if msaa == Msaa::Sample4
                        && !hdr
                        && modes.len() == 6
                        && !missing_attached
                        && !missing_depth
                    {
                        capture("world-ui-segmented", msaa.samples(), &expected);
                        capture("world-ui-shared", msaa.samples(), &actual);
                    }
                    assert!(
                        actual == expected,
                        "world UI changed: {msaa:?}, HDR={hdr}, {modes:?}, missing attached={missing_attached}, missing depth={missing_depth}"
                    );
                    assert!(
                        actual.as_chunks::<4>().0.iter().any(|pixel| pixel[2] > 0),
                        "UI did not draw: {msaa:?}, HDR={hdr}, {modes:?}, sample={:?}",
                        &actual[4 * (20 * SIDE as usize + 20)..4 * (20 * SIDE as usize + 20) + 4]
                    );
                }
            }
        }
    }
}
