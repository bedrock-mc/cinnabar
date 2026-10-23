//! Allocation lifetime and native-pixel raster checks for reduced-world composition.

use super::*;
use bevy::{
    asset::AssetPlugin,
    camera::{CameraMainTextureUsages, CameraPlugin, RenderTarget},
    core_pipeline::CorePipelinePlugin,
    ecs::system::RunSystemOnce,
    image::ImagePlugin,
    mesh::MeshPlugin,
    render::{
        RenderApp, RenderPlugin,
        camera::ExtractedCamera,
        renderer::{RenderInstance, RenderQueue},
        texture::{OutputColorAttachment, TextureCache},
        view::{ExtractedView, RetainedViewEntity, ViewTargetAttachments, prepare_view_targets},
    },
    window::WindowPlugin,
};

/// Starts only Bevy's core resources, with no camera or unrelated terrain pipeline warmup.
fn fixture(render: RenderPlugin) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins((AssetPlugin::default(), TransformPlugin))
        .add_plugins(render)
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ));
    super::super::upscale::install(&mut app);
    app.finish();
    app.cleanup();
    app.update();
    app
}

/// Attaches a real output while retaining independent low-resolution world metadata.
fn view(world: &mut World, output: [u32; 2], internal: [u32; 2]) -> Entity {
    let entity = world.spawn_empty().id();
    let target = RenderTarget::Window(bevy::window::WindowRef::Entity(entity))
        .normalize(None)
        .unwrap();
    let device = world.resource::<RenderDevice>();
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("native UI fixture output"),
        size: Extent3d {
            width: output[0],
            height: output[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: render_scene::SCENE_COLOR_FORMAT,
        // Deliberately not sampleable: the production path must never read its output.
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    world.init_resource::<ViewTargetAttachments>();
    world.resource_mut::<ViewTargetAttachments>().insert(
        target.clone(),
        OutputColorAttachment::new(texture.create_view(&default()), texture.format()),
    );
    world.init_resource::<TextureCache>();
    world.init_resource::<ClearColor>();
    let mut camera = crate::render_test_support::camera(false);
    camera.target = Some(target);
    camera.physical_target_size = Some(UVec2::from_array(internal));
    camera.physical_viewport_size = camera.physical_target_size;
    world.entity_mut(entity).insert((
        camera,
        ExtractedView {
            retained_view_entity: RetainedViewEntity::new(entity.into(), None, 0),
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::default(),
            clip_from_world: None,
            target_format: render_scene::SCENE_COLOR_FORMAT,
            viewport: UVec4::new(0, 0, internal[0], internal[1]),
            color_grading: default(),
            invert_culling: false,
        },
        CameraMainTextureUsages(
            TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST,
        ),
        Msaa::Off,
        crate::render_scale::RenderScaleOutput(UVec2::from_array(output)),
    ));
    prepare(world);
    entity
}

/// Applies the same ordered view-target and native-target preparation as the UI plugin.
fn prepare(world: &mut World) {
    world.run_system_once(prepare_view_targets).unwrap();
    world.run_system_once(prepare_native_ui_targets).unwrap();
}

#[test]
fn native_ui_pair_keeps_output_pixels_and_reuses_across_world_scale_changes() {
    let mut app = fixture(crate::chunk::noop_render_plugin(wgpu::Features::empty()));
    let world = app.sub_app_mut(RenderApp).world_mut();
    let entity = view(world, [81, 61], [8, 6]);
    let original = world
        .get::<NativeUiTarget>(entity)
        .unwrap()
        .textures
        .each_ref()
        .map(Texture::id);
    for internal in [[8, 6], [40, 30], [54, 41], [8, 6]] {
        let mut camera = world.get_mut::<ExtractedCamera>(entity).unwrap();
        camera.physical_target_size = Some(UVec2::from_array(internal));
        camera.physical_viewport_size = camera.physical_target_size;
        prepare(world);
        let target = world.get::<ViewTarget>(entity).unwrap();
        let native = world.get::<NativeUiTarget>(entity).unwrap();
        assert_eq!(native.textures.each_ref().map(Texture::id), original);
        assert_eq!(native.extent(), [81, 61]);
        assert!(!native.is_ready() && !native.was_presented());
        native.mark_presented();
        assert!(
            !native.was_presented(),
            "unseeded UI cannot suppress the fallback blit"
        );
        let low = target.main_texture().id();
        assert_eq!(
            [
                target.main_texture().width(),
                target.main_texture().height()
            ],
            internal
        );
        let ui = UiTarget::new(target, Some(native));
        assert!(ui.is_native());
        assert_eq!(
            [ui.main_texture().width(), ui.main_texture().height()],
            [81, 61]
        );
        assert_eq!(ui.main_texture_other().id(), native.textures[1].id());
        let a = ui.main_texture_view().id();
        let b = ui.main_texture_other_view().id();
        let write = ui.post_process_write();
        assert_eq!((write.source.id(), write.destination.id()), (a, b));
        assert_eq!(ui.main_texture_view().id(), b);
        assert_eq!(
            target.main_texture().id(),
            low,
            "UI ping-pong never flips world history"
        );
        native.ready.store(true, Ordering::Relaxed);
        native.mark_presented();
        assert!(native.was_presented());
    }
    prepare(world);
    let native = world.get::<NativeUiTarget>(entity).unwrap();
    assert!(!native.is_ready() && !native.was_presented());
    world
        .entity_mut(entity)
        .remove::<crate::render_scale::RenderScaleOutput>();
    world.run_system_once(prepare_native_ui_targets).unwrap();
    assert!(world.get::<NativeUiTarget>(entity).is_none());
}

#[test]
fn native_ui_target_rejects_stale_output_and_releases_removed_views() {
    let mut app = fixture(crate::chunk::noop_render_plugin(wgpu::Features::empty()));
    let world = app.sub_app_mut(RenderApp).world_mut();
    let entity = view(world, [81, 61], [8, 6]);
    world
        .entity_mut(entity)
        .insert(crate::render_scale::RenderScaleOutput(UVec2::new(80, 60)));
    world.run_system_once(prepare_native_ui_targets).unwrap();
    assert!(world.get::<NativeUiTarget>(entity).is_none());
    world
        .entity_mut(entity)
        .insert(crate::render_scale::RenderScaleOutput(UVec2::new(81, 61)));
    world.run_system_once(prepare_native_ui_targets).unwrap();
    assert!(world.get::<NativeUiTarget>(entity).is_some());
    world.entity_mut(entity).remove::<ViewTarget>();
    world.run_system_once(prepare_native_ui_targets).unwrap();
    assert!(world.get::<NativeUiTarget>(entity).is_none());
}

#[test]
fn screen_ui_layers_stay_native_at_ten_and_fifty_percent_and_follow_output_resize() {
    use super::super::composite::{UiLayerStore, UiLayerTexture, prepare_ui_layers};
    let mut app = fixture(crate::chunk::noop_render_plugin(wgpu::Features::empty()));
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.init_resource::<UiLayerStore>();
    let entity = view(world, [81, 61], [8, 6]);
    world.run_system_once(prepare_ui_layers).unwrap();
    let original_layer = world.get::<UiLayerTexture>(entity).unwrap().texture.id();
    let original_native = world.get::<NativeUiTarget>(entity).unwrap().textures[0].id();
    for internal in [[8, 6], [41, 31], [8, 6]] {
        let mut camera = world.get_mut::<ExtractedCamera>(entity).unwrap();
        camera.physical_target_size = Some(UVec2::from_array(internal));
        camera.physical_viewport_size = camera.physical_target_size;
        prepare(world);
        world.run_system_once(prepare_ui_layers).unwrap();
        let layer = world.get::<UiLayerTexture>(entity).unwrap();
        assert_eq!([layer.texture.width(), layer.texture.height()], [81, 61]);
        assert_eq!(
            layer.texture.id(),
            original_layer,
            "world scale must not reallocate native UI layers"
        );
        assert_eq!(
            world.get::<NativeUiTarget>(entity).unwrap().textures[0].id(),
            original_native
        );
    }
    let output = [121, 91];
    let texture = world
        .resource::<RenderDevice>()
        .create_texture(&TextureDescriptor {
            label: Some("resized native UI output"),
            size: Extent3d {
                width: output[0],
                height: output[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: render_scene::SCENE_COLOR_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
    let target = world
        .get::<ExtractedCamera>(entity)
        .unwrap()
        .target
        .clone()
        .unwrap();
    world.resource_mut::<ViewTargetAttachments>().insert(
        target,
        OutputColorAttachment::new(texture.create_view(&default()), texture.format()),
    );
    world
        .entity_mut(entity)
        .insert(crate::render_scale::RenderScaleOutput(UVec2::from_array(
            output,
        )));
    for internal in [[12, 9], [61, 46]] {
        let mut camera = world.get_mut::<ExtractedCamera>(entity).unwrap();
        camera.physical_target_size = Some(UVec2::from_array(internal));
        camera.physical_viewport_size = camera.physical_target_size;
        prepare(world);
        world.run_system_once(prepare_ui_layers).unwrap();
        let layer = world.get::<UiLayerTexture>(entity).unwrap();
        assert_eq!([layer.texture.width(), layer.texture.height()], output);
        assert_ne!(
            layer.texture.id(),
            original_layer,
            "native output resize replaces the UI raster"
        );
        assert_ne!(
            world.get::<NativeUiTarget>(entity).unwrap().textures[0].id(),
            original_native
        );
        let low = world.get::<ViewTarget>(entity).unwrap().main_texture();
        assert_eq!(
            [low.width(), low.height()],
            internal,
            "world keeps its independent render extent"
        );
    }
}

/// Reads output pixels using padded GPU copy rows, without another rendering update.
fn pixels(world: &World, entity: Entity) -> Vec<[u8; 4]> {
    let device = world.resource::<RenderDevice>();
    let queue = world.resource::<RenderQueue>();
    let target = world.get::<ViewTarget>(entity).unwrap();
    let output = target.out_texture().unwrap().texture();
    let row_bytes = (output.width() * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("native UI raster readback"),
        size: u64::from(row_bytes) * u64::from(output.height()),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(output.height()),
            },
        },
        output.size(),
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    buffer
        .slice(..)
        .get_mapped_range()
        .unwrap()
        .chunks_exact(row_bytes as usize)
        .flat_map(|row| {
            row[..output.width() as usize * 4]
                .chunks_exact(4)
                .map(|pixel| pixel.try_into().unwrap())
        })
        .collect()
}

#[test]
fn native_one_pixel_hud_survives_ten_percent_world_upscale_and_empty_next_frame() {
    for sharpen in [false, true] {
        native_one_pixel_hud(sharpen);
    }
}

/// Checks both filters through real GPU draws and output readback, including an empty next HUD.
fn native_one_pixel_hud(sharpen: bool) {
    let Some(renderer) = crate::render_test_support::scene_raster::renderer() else {
        return;
    };
    let mut app = fixture(renderer);
    let shaders = {
        let render = app.sub_app(RenderApp).world();
        let blit = render.resource::<BlitPipeline>();
        [
            blit.fullscreen_shader.shader(),
            blit.fragment_shader.clone(),
        ]
    };
    test_time::eventually("native UI fixture blit shaders", || {
        app.update();
        let assets = app.world().resource::<bevy::asset::AssetServer>();
        shaders
            .iter()
            .all(|shader| assets.is_loaded_with_dependencies(shader.id()))
    });
    app.update();
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.insert_resource(crate::render_scale::RenderScaleSharpen(sharpen));
    let entity = view(world, [40, 30], [4, 3]);
    world.resource_mut::<PipelineCache>().process_queue();
    let device = world.resource::<RenderDevice>().clone();
    {
        let adapter = bevy::tasks::block_on(
            world
                .resource::<RenderInstance>()
                .request_adapter(&default()),
        )
        .expect("replacement native UI fixture adapter");
        let (replacement, _queue) =
            bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_limits: adapter.limits(),
                ..default()
            }))
            .expect("replacement native UI fixture device");
        let native = world.get::<NativeUiTarget>(entity).unwrap();
        let target = world.get::<ViewTarget>(entity).unwrap();
        assert!(native.matches(&device, target, [40, 30]));
        assert!(
            !native.matches(&RenderDevice::from(replacement), target, [40, 30]),
            "native UI resources cannot survive device replacement"
        );
    }
    let source: Vec<[u8; 4]> = (0..3)
        .flat_map(|y| (0..4).map(move |x| [20 + x * 50, 40 + y * 50, 60, 255]))
        .collect();
    let shader = device
        .wgpu_device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("one output pixel HUD witness"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4(vec2(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - vec2(1.0), 0.0, 1.0);
}
@fragment fn fragment() -> @location(0) vec4<f32> { return vec4(1.0); }
"#
                .into(),
            ),
        });
    let hud = device
        .wgpu_device()
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("one native HUD pixel"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: default(),
            depth_stencil: None,
            multisample: default(),
            multiview_mask: None,
            cache: None,
        });
    let mut previous = None;
    for draw_hud in [true, false] {
        world.run_system_once(prepare_native_ui_targets).unwrap();
        let target = world.get::<ViewTarget>(entity).unwrap();
        let native = world.get::<NativeUiTarget>(entity).unwrap();
        assert!(!native.is_ready() && !native.was_presented());
        world.resource::<RenderQueue>().write_texture(
            target.main_texture().as_image_copy(),
            bytemuck::cast_slice(&source),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: Some(3),
            },
            Extent3d {
                width: 4,
                height: 3,
                depth_or_array_layers: 1,
            },
        );
        let (_, commands) = crate::render_test_support::record(world, &device, |world, context| {
            let target = world.get::<ViewTarget>(entity).unwrap();
            let native = world.get::<NativeUiTarget>(entity).unwrap();
            assert!(!native.present(world, context, target));
            assert!(native.seed(world, context, target));
            if draw_hud {
                let view = UiTarget::new(target, Some(native)).main_texture_view();
                let attachments = [Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let mut pass =
                    context
                        .command_encoder()
                        .begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("native one-pixel HUD"),
                            color_attachments: &attachments,
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                pass.set_pipeline(&hud);
                pass.set_scissor_rect(17, 3, 1, 1);
                pass.draw(0..3, 0..1);
            }
            assert!(native.present(world, context, target));
            assert!(native.was_presented());
            assert!(
                target.needs_present(),
                "native output must pass Bevy's surface presentation gate"
            );
        });
        world.resource::<RenderQueue>().submit(commands);
        let actual = pixels(world, entity);
        if draw_hud {
            assert_eq!(actual[3 * 40 + 17], [255; 4]);
        }
        // The gradient must interpolate inside each source texel rather than forming blocks.
        assert!(actual[10 * 40 + 14][0] < actual[10 * 40 + 15][0]);
        assert!(actual[10 * 40 + 14][0] > source[0][0]);
        for (index, pixel) in actual.iter().enumerate() {
            if draw_hud && index == 3 * 40 + 17 {
                continue;
            }
            assert!(
                pixel[2].abs_diff(60) <= 1,
                "uniform blue must survive filtering"
            );
            assert_eq!(pixel[3], 255);
            if index != 3 * 40 + 17
                && let Some(before) = &previous
            {
                let before: &Vec<[u8; 4]> = before;
                assert_eq!(*pixel, before[index], "only the HUD pixel may change");
            }
        }
        previous = Some(actual);
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

#[test]
fn absent_blit_resources_still_release_unscaled_native_targets() {
    let mut app = fixture(crate::chunk::noop_render_plugin(wgpu::Features::empty()));
    let world = app.sub_app_mut(RenderApp).world_mut();
    let entity = view(world, [81, 61], [8, 6]);
    world.remove_resource::<BlitPipeline>();
    world.remove_resource::<SpecializedRenderPipelines<BlitPipeline>>();
    world
        .entity_mut(entity)
        .remove::<crate::render_scale::RenderScaleOutput>();
    world.run_system_once(prepare_native_ui_targets).unwrap();
    assert!(world.get::<NativeUiTarget>(entity).is_none());
    world
        .entity_mut(entity)
        .insert(crate::render_scale::RenderScaleOutput(UVec2::new(81, 61)));
    world.run_system_once(prepare_native_ui_targets).unwrap();
    assert!(
        world.get::<NativeUiTarget>(entity).is_none(),
        "missing blit resources cannot admit an unseedable native scene"
    );
}

/// An allocation failure must not publish unusable textures or keep a stale native target.
#[test]
fn failed_native_ui_allocation_withholds_the_target() {
    let mut app = fixture(crate::chunk::noop_render_plugin(wgpu::Features::empty()));
    let world = app.sub_app_mut(RenderApp).world_mut();
    let _failure = crate::gpu_allocation::inject(|label, _| label == "native-resolution UI scene");
    let entity = view(world, [81, 61], [8, 6]);
    assert!(world.get::<NativeUiTarget>(entity).is_none());
}

/// Native scene allocation retries after memory pressure without blocking every frame.
#[test]
fn native_ui_allocation_retries_after_backoff_and_recovers() {
    use std::{cell::Cell, rc::Rc};
    let mut app = fixture(crate::chunk::noop_render_plugin(wgpu::Features::empty()));
    let world = app.sub_app_mut(RenderApp).world_mut();
    let entity = view(world, [81, 61], [8, 6]);
    world.entity_mut(entity).remove::<NativeUiTarget>();
    let attempts = Rc::new(Cell::new(0));
    let observed = Rc::clone(&attempts);
    let failure = crate::gpu_allocation::inject(move |label, _| {
        if label == "native-resolution UI scene" {
            observed.set(observed.get() + 1);
            true
        } else {
            false
        }
    });
    let mut system = IntoSystem::into_system(prepare_native_ui_targets);
    system.initialize(world);
    for expected in [1, 1, 1, 2] {
        system.run((), world).unwrap();
        assert_eq!(attempts.get(), expected);
        assert!(world.get::<NativeUiTarget>(entity).is_none());
    }
    drop(failure);
    for _ in 0..5 {
        system.run((), world).unwrap();
    }
    assert!(world.get::<NativeUiTarget>(entity).is_some());
}

/// HDR world targets keep their format while native UI retains the output's dimensions.
#[test]
fn native_ui_supports_scaled_hdr_views() {
    let mut app = fixture(crate::chunk::noop_render_plugin(wgpu::Features::empty()));
    let world = app.sub_app_mut(RenderApp).world_mut();
    let entity = view(world, [81, 61], [40, 30]);
    world.get_mut::<ExtractedCamera>(entity).unwrap().hdr = true;
    world
        .get_mut::<ExtractedView>(entity)
        .unwrap()
        .target_format = render_scene::SCENE_HDR_FORMAT;
    prepare(world);
    let native = world.get::<NativeUiTarget>(entity).unwrap();
    assert_eq!(native.extent(), [81, 61]);
    assert!(
        native
            .textures
            .iter()
            .all(|texture| texture.format() == render_scene::SCENE_HDR_FORMAT)
    );
    assert_eq!(
        world
            .get::<ViewTarget>(entity)
            .unwrap()
            .main_texture()
            .width(),
        40
    );
}
