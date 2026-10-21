//! Actual spatial filtering and exposure compose over a stationary diagonal actor silhouette.

use super::super::*;
use crate::depth_smaa::tests::raster_tests::{app_with_effects, ready};
use crate::render_test_support::scene_raster::{
    FixturePipelines, SIDE, fixture_pipeline, frame, read_pixels, renderer,
};
use bevy::{
    anti_alias::smaa::Smaa,
    render::{Render, RenderApp, RenderSystems},
};

#[derive(Resource)]
struct StationaryCoverage {
    view: TextureView,
    enabled: bool,
}

/// Supplies zero actor velocity over the exact opaque silhouette to the real exposure node.
fn stationary_motion_bindings(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    pipeline: Res<BlurPipeline>,
    coverage: Res<StationaryCoverage>,
    mut views: Query<(&mut BlurView, &ViewTarget, &ViewDepthStencilTexture)>,
) {
    for (mut state, target, depth) in &mut views {
        let Some(mut uniform) = state.last_uniform else {
            continue;
        };
        uniform.strength.z = if coverage.enabled { 1.0 } else { 0.0 };
        queue.write_buffer(&state.uniform, 0, bytemuck::bytes_of(&uniform));
        state.last_uniform = Some(uniform);
        let uniform_buffer = state.uniform.clone();
        super::super::super::resolve::ResolvedInputs::prepare(
            &mut state.resolved,
            &device,
            &cache,
            pipeline.resolve_layout(1),
            &uniform_buffer,
            (crate::scene_sampling::view_depth(depth), &coverage.view),
            target.main_texture().size(),
        );
        let Some(resolved) = &state.resolved else {
            continue;
        };
        let Some(footprint) = &state.footprint else {
            continue;
        };
        let motion = &coverage.view;
        let depth = crate::scene_sampling::view_depth(depth);
        let bindings =
            [target.main_texture_view(), target.main_texture_other_view()].map(|source| {
                let binding = device.create_bind_group(
                    "stationary actor composed fixture",
                    &cache.get_bind_group_layout(pipeline.layout()),
                    &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::TextureView(source),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::Sampler(&pipeline.sampler),
                        },
                        BindGroupEntry {
                            binding: 3,
                            resource: state.uniform.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 5,
                            resource: BindingResource::TextureView(&footprint.view),
                        },
                        BindGroupEntry {
                            binding: 6,
                            resource: BindingResource::TextureView(&resolved.motion),
                        },
                        BindGroupEntry {
                            binding: 7,
                            resource: BindingResource::TextureView(&resolved.depth),
                        },
                    ],
                );
                (source.id(), depth.id(), motion.id(), binding)
            });
        state.bindings = bindings.into();
    }
}

/// Rotates the camera across rendered frames while the supplied actor stays fixed on screen.
fn moving_frames(app: &mut App, camera: Entity) {
    for _ in 0..8 {
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .rotate_y(0.06);
        frame(app);
    }
}

#[test]
fn camera_followed_diagonal_actor_keeps_smaa_edges_sharp_with_exposure() {
    let Some(render) = renderer() else {
        return;
    };
    let (mut app, camera, image) = app_with_effects(render, |app| {
        app.insert_resource(render_scene::RuntimeStageProfiler::new(false));
        app.add_plugins((
            crate::ui_render::UiRenderPlugin,
            super::super::super::CameraMotionBlurPlugin,
            crate::GpuTimingPlugin,
        ));
    });
    let world = app.sub_app_mut(RenderApp).world_mut();
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    world.insert_resource(FixturePipelines {
        checker: fixture_pipeline(&device, 1, "checker"),
        sloped_checker: fixture_pipeline(&device, 1, "sloped_checker"),
        silhouette: fixture_pipeline(&device, 1, "silhouette"),
        scene: 2,
    });
    let coverage = device.create_texture(&TextureDescriptor {
        label: Some("stationary diagonal actor coverage"),
        size: Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba32Float,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut texels = Vec::new();
    for y in 0..SIDE {
        for x in 0..SIDE {
            let covered: f32 = if x as f32 + 0.5 + (y as f32 + 0.5) * 0.6 < 49.0 {
                1.0
            } else {
                0.0
            };
            texels.extend_from_slice(&[0.0, 0.0, covered, covered]);
        }
    }
    queue.write_texture(
        coverage.as_image_copy(),
        bytemuck::cast_slice(&texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SIDE * 16),
            rows_per_image: Some(SIDE),
        },
        coverage.size(),
    );
    world.insert_resource(StationaryCoverage {
        view: coverage.create_view(&default()),
        enabled: true,
    });
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        stationary_motion_bindings
            .after(prepare_views)
            .in_set(RenderSystems::PrepareBindGroups),
    );
    moving_frames(&mut app, camera);
    let unfiltered = read_pixels(&app, &image);
    app.world_mut().entity_mut(camera).insert(Smaa::default());
    ready(&mut app);
    let spatial = read_pixels(&app, &image);
    assert_ne!(
        spatial, unfiltered,
        "real SMAA must filter the diagonal depth silhouette"
    );
    assert!(
        spatial
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] > 0 && pixel[0] < 255)
    );
    app.world_mut().entity_mut(camera).insert(CameraMotionBlur {
        exposure_seconds: 0.02,
        delta_seconds: 0.02,
        samples: 15,
        reset_epoch: 0,
    });
    moving_frames(&mut app, camera);
    let followed = read_pixels(&app, &image);
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<StationaryCoverage>()
        .enabled = false;
    moving_frames(&mut app, camera);
    let world_exposure = read_pixels(&app, &image);
    assert_ne!(
        world_exposure, spatial,
        "real camera exposure must remain active for world geometry"
    );
    assert_eq!(
        followed, spatial,
        "camera motion smeared SMAA colour beyond a stationary actor's raw silhouette"
    );
}
