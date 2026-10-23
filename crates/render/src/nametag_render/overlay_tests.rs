//! Runs the production label passes around a world colour replacement.

use super::*;
use crate::render_test_support::scene_raster::{app_with_effects, frame, read_pixels, renderer};
use bevy::{
    core_pipeline::{Core3d, Core3dSystems},
    prelude::*,
    render::{render_resource::*, renderer::RenderContext, view::ViewTarget},
};

#[derive(Resource)]
struct Grade(wgpu::RenderPipeline, bool);

/// Replaces world colour as a final shader-pack pass would, without touching scene depth.
fn grade(
    world: &World,
    query: bevy::render::renderer::ViewQuery<&'static ViewTarget>,
    mut context: RenderContext,
) {
    if !world.resource::<Grade>().1 {
        return;
    }
    let target = query.into_inner();
    let post = target.post_process_write();
    let attachments = [Some(RenderPassColorAttachment {
        view: post.destination,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    })];
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("world colour replacement fixture"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(&world.resource::<Grade>().0);
    pass.draw(0..3, 0..1);
}

/// Models an authored mod effect sharing the view's alternating scene textures.
fn mod_grade(
    world: &World,
    query: bevy::render::renderer::ViewQuery<&'static ViewTarget>,
    context: RenderContext,
) {
    grade(world, query, context);
}

/// Models selection-halo composition after the authored effects.
fn halo_grade(
    world: &World,
    query: bevy::render::renderer::ViewQuery<&'static ViewTarget>,
    context: RenderContext,
) {
    grade(world, query, context);
}

/// Builds known scene geometry and the production nametag phase without game carriers.
fn fixture(hdr: bool, msaa: Msaa) -> Option<(App, Entity, Handle<Image>)> {
    let (mut app, camera, image) = app_with_effects(renderer()?, |app| {
        app.add_plugins(bevy::render::extract_component::ExtractComponentPlugin::<
            render_scene::shaderpack::ShaderPackRendering,
        >::default());
        crate::lighting::install(app);
        install_nametag_render(app);
        crate::chunk::transparent::gamma_pass::register_fixture_commands(app);
    });
    if hdr {
        app.world_mut().entity_mut(camera).insert(bevy::camera::Hdr);
    }
    app.world_mut()
        .entity_mut(camera)
        .insert((render_scene::shaderpack::ShaderPackRendering, msaa));
    let world = app.sub_app_mut(RenderApp).world_mut();
    let device = world.resource::<RenderDevice>().clone();
    use crate::render_test_support::scene_raster::FixturePipelines;
    let format = if hdr {
        render_scene::SCENE_HDR_FORMAT
    } else {
        render_scene::SCENE_COLOR_FORMAT
    };
    let module = device
        .wgpu_device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world colour replacement"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
// Both paths start with the same world colour for a complete coverage comparison.
fn background() -> vec4<f32> { return vec4(0.04, 0.08, 0.12, 1.0); }
@fragment fn fragment() -> @location(0) vec4<f32> { return background(); }
struct WorldOutput { @location(0) colour: vec4<f32>, @builtin(frag_depth) depth: f32 }
@fragment fn world_fragment() -> WorldOutput { return WorldOutput(background(), 0.5); }
"#
                .into(),
            ),
        });
    let pipeline = |samples: u32, entry: &str, depth: bool| {
        device
            .wgpu_device()
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world colour replacement"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vertex"),
                    buffers: &[],
                    compilation_options: default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                primitive: default(),
                depth_stencil: depth.then_some(wgpu::DepthStencilState {
                    format: bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: default(),
                    bias: default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..default()
                },
                multiview_mask: None,
                cache: None,
            })
    };
    let opaque = pipeline(msaa.samples(), "world_fragment", true);
    world.insert_resource(FixturePipelines {
        checker: opaque.clone(),
        sloped_checker: opaque.clone(),
        silhouette: opaque,
        scene: 0,
    });
    world.insert_resource(Grade(pipeline(1, "fragment", false), true));
    crate::chunk::transparent::gamma_pass::install_graph(world);
    overlay::install_graph(world);
    world.schedule_scope(Core3d, |_, schedule| {
        schedule.add_systems((
            grade
                .in_set(Core3dSystems::PostProcess)
                .before(bevy::core_pipeline::tonemapping::tonemapping),
            mod_grade
                .in_set(crate::mod_render::ModPassLabel)
                .after(Core3dSystems::PostProcess),
            halo_grade
                .in_set(crate::mod_render::composition::ModCompositionLabel)
                .after(Core3dSystems::PostProcess)
                .after(crate::mod_render::ModPassLabel),
        ));
    });
    Some((app, camera, image))
}

/// Reads a settled frame after pipeline preparation and resource extraction.
fn pixels(app: &mut App, image: &Handle<Image>) -> Vec<u8> {
    for _ in 0..8 {
        frame(app);
    }
    read_pixels(app, image)
}

/// Saves requested native raster evidence beside the other review snapshots.
fn save(name: &str, pixels: &[u8]) {
    if let Some(directory) = std::env::var_os("CINNABAR_REVIEW_SNAPSHOT_DIR") {
        image::save_buffer(
            std::path::PathBuf::from(directory).join(format!("{name}.png")),
            pixels,
            crate::render_test_support::scene_raster::SIDE,
            crate::render_test_support::scene_raster::SIDE,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}

/// Supplies a flat label large enough to inspect its centre and plate margins.
fn label(depth_tested: bool) -> NametagRecord {
    NametagRecord {
        anchor: [0., 0., -2.],
        rect: [-16., -8., 16., 8.],
        uv: [0., 0., -1., -1.],
        color: [0.5, 0.25, 0.75, 1.],
        depth_tested: u32::from(depth_tested),
        text: 1,
        ..default()
    }
}

#[test]
#[cfg(feature = "shaderpacks")]
fn labels_keep_their_colour_after_ldr_and_hdr_world_replacement_and_retain_depth() {
    use crate::render_test_support::scene_raster::SIDE;
    for (hdr, reduced, msaa) in [false, true].into_iter().flat_map(|hdr| {
        [false, true]
            .into_iter()
            .flat_map(move |reduced| [Msaa::Off, Msaa::Sample4].map(|msaa| (hdr, reduced, msaa)))
    }) {
        let Some((mut app, camera, image)) = fixture(hdr, msaa) else {
            return;
        };
        if reduced {
            app.world_mut()
                .entity_mut(camera)
                .insert(bevy::camera::MainPassResolutionOverride(UVec2::splat(
                    SIDE / 2,
                )));
        }
        let background = pixels(&mut app, &image);
        app.world_mut()
            .resource_mut::<NametagSceneResource>()
            .records = vec![label(false)];
        let visible = pixels(&mut app, &image);
        save(
            &format!(
                "nametag-world-hdr-{hdr}-reduced-{reduced}-samples-{}",
                msaa.samples()
            ),
            &visible,
        );
        let center = ((SIDE / 2 * SIDE + SIDE / 2) * 4) as usize;
        app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<Grade>()
            .1 = false;
        app.world_mut()
            .entity_mut(camera)
            .remove::<render_scene::shaderpack::ShaderPackRendering>();
        let native = pixels(&mut app, &image);
        if let Some(byte) = visible
            .iter()
            .zip(&native)
            .position(|(actual, expected)| actual.abs_diff(*expected) > 2)
        {
            let at = byte / 4 * 4;
            panic!(
                "hdr {hdr}, reduced {reduced}, msaa {msaa:?}, pixel {}: overlay {:?}, native {:?}",
                byte / 4,
                &visible[at..at + 4],
                &native[at..at + 4]
            );
        }
        app.world_mut()
            .entity_mut(camera)
            .insert(render_scene::shaderpack::ShaderPackRendering);
        app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<Grade>()
            .1 = true;
        assert_eq!(
            &visible[..4],
            &background[..4],
            "empty overlay pixels retain the scene"
        );
        app.world_mut()
            .resource_mut::<NametagSceneResource>()
            .records = vec![label(true)];
        let occluded = pixels(&mut app, &image);
        assert_eq!(
            &occluded[center..center + 4],
            &background[center..center + 4],
            "sneaking ink tests the retained scene depth"
        );
        app.world_mut()
            .resource_mut::<NametagSceneResource>()
            .records
            .clear();
        assert_eq!(
            pixels(&mut app, &image),
            background,
            "removing all tags leaves no stale overlay"
        );
    }
}

#[test]
fn ordinary_views_do_not_warm_shader_pack_label_variants() {
    use crate::pipeline_warmup::{PrewarmPipelines, WarmView};
    let (device, _) = wgpu::Device::noop(&default());
    let cache = PipelineCache::new(RenderDevice::from(device), true);
    let mut pipeline = NametagPipeline::from_world(&mut World::new());
    let mut ids = Vec::new();
    let view = WarmView {
        msaa: Msaa::Off,
        hdr: true,
        enhanced: false,
        shader_pack: false,
        output: None,
    };
    pipeline.prewarm(&cache, view, &mut ids).unwrap();
    assert_eq!(ids.len(), 4);
    ids.clear();
    pipeline
        .prewarm(
            &cache,
            WarmView {
                shader_pack: true,
                ..view
            },
            &mut ids,
        )
        .unwrap();
    assert_eq!(ids.len(), if cfg!(feature = "shaderpacks") { 8 } else { 4 });
}

/// Leaving the pack releases the full-screen label layer; returning prepares a fresh one.
#[test]
#[cfg(feature = "shaderpacks")]
fn leaving_the_pack_releases_the_label_overlay() {
    use bevy::{
        camera::{CameraMainTextureUsages, ClearColorConfig},
        ecs::system::RunSystemOnce,
        render::{
            texture::TextureCache,
            view::{
                ExtractedView, RetainedViewEntity, ViewDepthStencilTexture, ViewTargetAttachments,
                prepare_view_targets,
            },
        },
    };
    use render_scene::shaderpack::ShaderPackRendering;

    let (device, _) = wgpu::Device::noop(&default());
    let device = RenderDevice::from(device);
    let mut world = World::new();
    world.insert_resource(device.clone());
    world.init_resource::<ClearColor>();
    world.init_resource::<TextureCache>();
    world.init_resource::<ViewTargetAttachments>();
    let depth = world.resource_mut::<TextureCache>().get(
        &device,
        TextureDescriptor {
            label: None,
            size: Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
    );
    let mut camera = crate::render_test_support::camera(false);
    camera.physical_target_size = Some(UVec2::splat(4));
    camera.clear_color = ClearColorConfig::None;
    let view = world
        .spawn((
            camera,
            CameraMainTextureUsages::default(),
            Msaa::Off,
            ViewDepthStencilTexture::new(depth, Some(0.0), None),
            ExtractedView {
                retained_view_entity: RetainedViewEntity::new(Entity::PLACEHOLDER.into(), None, 0),
                clip_from_view: Mat4::IDENTITY,
                world_from_view: GlobalTransform::default(),
                clip_from_world: None,
                target_format: render_scene::SCENE_COLOR_FORMAT,
                viewport: UVec4::new(0, 0, 4, 4),
                color_grading: default(),
                invert_culling: false,
            },
        ))
        .id();
    world.run_system_once(prepare_view_targets).unwrap();
    for pack in [false, true, false, true] {
        if pack {
            world.entity_mut(view).insert(ShaderPackRendering);
        } else {
            world.entity_mut(view).remove::<ShaderPackRendering>();
        }
        world.run_system_once(overlay::prepare).unwrap();
        assert_eq!(world.entity(view).contains::<overlay::Overlay>(), pack);
    }
}
