//! Deterministic resource and allocation contracts for the optional exposure pass.

use super::{CameraMotionBlur, pipeline::BlurPipeline, prepare::BlurView};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::ecs::system::RunSystemOnce;

use bevy::{
    camera::{
        CameraMainTextureUsages, CameraOutputMode, ClearColorConfig, MsaaWriteback,
        NormalizedRenderTarget, RenderTarget,
    },
    core_pipeline::Core3d,
    prelude::*,
    render::{
        camera::ExtractedCamera,
        render_resource::*,
        renderer::RenderDevice,
        texture::{CachedTexture, OutputColorAttachment, TextureCache},
        view::{ViewDepthStencilTexture, ViewTarget, ViewTargetAttachments, prepare_view_targets},
    },
};

pub(super) fn fixture() -> (App, Entity) {
    let (mut app, retained) = crate::queue_review_support::app();
    let world = app.world_mut();
    let entity = retained.main_entity.id();
    let device = world.resource::<RenderDevice>().clone();
    let texture = |format, usage| {
        device.create_texture(&TextureDescriptor {
            label: Some("camera exposure resource fixture"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let output = texture(
        TextureFormat::Bgra8UnormSrgb,
        TextureUsages::RENDER_ATTACHMENT,
    );
    let target: NormalizedRenderTarget =
        RenderTarget::Window(bevy::window::WindowRef::Entity(Entity::PLACEHOLDER))
            .normalize(None)
            .unwrap();
    let mut attachments = ViewTargetAttachments::default();
    attachments.insert(
        target.clone(),
        OutputColorAttachment::new(output.create_view(&Default::default()), output.format()),
    );
    world.insert_resource(attachments);
    world.init_resource::<TextureCache>();
    world.init_resource::<ClearColor>();
    let depth = texture(
        TextureFormat::Depth32Float,
        TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
    );
    let depth_view = depth.create_view(&Default::default());
    world.entity_mut(entity).insert((
        Camera3d::default(),
        CameraMainTextureUsages::default(),
        Msaa::Off,
        ExtractedCamera {
            target: Some(target),
            physical_viewport_size: Some(UVec2::ONE),
            physical_target_size: Some(UVec2::ONE),
            viewport: None,
            schedule: Core3d.intern(),
            order: 0,
            output_mode: CameraOutputMode::default(),
            msaa_writeback: MsaaWriteback::default(),
            clear_color: ClearColorConfig::default(),
            sorted_camera_index_for_target: 0,
            exposure: 1.0,
            hdr: false,
        },
        ViewDepthStencilTexture::new(
            CachedTexture {
                texture: depth,
                default_view: depth_view,
            },
            Some(0.0),
            None,
        ),
    ));
    world.run_system_once(prepare_view_targets).unwrap();
    world.run_system_once(super::pipeline::init).unwrap();
    (app, entity)
}

#[test]
fn motion_blur_off_allocates_no_view_resources_and_releases_enabled_resources() {
    let (mut app, entity) = fixture();
    let world = app.world_mut();
    let mut prepare = IntoSystem::into_system(super::prepare::prepare_views);
    prepare.initialize(world);
    prepare.run((), world).unwrap();
    let allocated = test_alloc::thread_allocations();
    prepare.run((), world).unwrap();
    assert_eq!(test_alloc::thread_allocations(), allocated);
    assert!(world.get::<BlurView>(entity).is_none());

    world.entity_mut(entity).insert(CameraMotionBlur {
        exposure_seconds: 0.01,
        delta_seconds: 0.01,
        samples: 7,
        reset_epoch: 0,
    });
    prepare.run((), world).unwrap();
    let target = world
        .get::<ViewTarget>(entity)
        .unwrap()
        .main_texture_view()
        .id();
    let depth =
        crate::scene_sampling::view_depth(world.get::<ViewDepthStencilTexture>(entity).unwrap())
            .id();
    let binding = world
        .get::<BlurView>(entity)
        .unwrap()
        .binding(target, depth)
        .unwrap()
        .id();
    let tiles = world
        .get::<BlurView>(entity)
        .unwrap()
        .footprint
        .as_ref()
        .unwrap()
        .view
        .id();
    prepare.run((), world).unwrap();
    let allocated = test_alloc::thread_allocations();
    prepare.run((), world).unwrap();
    assert_eq!(test_alloc::thread_allocations(), allocated);
    assert_eq!(
        world
            .get::<BlurView>(entity)
            .unwrap()
            .footprint
            .as_ref()
            .unwrap()
            .view
            .id(),
        tiles
    );
    assert_eq!(
        world
            .get::<BlurView>(entity)
            .unwrap()
            .binding(target, depth)
            .unwrap()
            .id(),
        binding
    );

    world.entity_mut(entity).remove::<CameraMotionBlur>();
    prepare.run((), world).unwrap();
    assert!(world.get::<BlurView>(entity).is_none());
    let allocated = test_alloc::thread_allocations();
    prepare.run((), world).unwrap();
    assert_eq!(test_alloc::thread_allocations(), allocated);
    assert!(!super::applies(world, entity));
}

#[test]
fn motion_blur_prepares_both_scene_colours_before_smaa_flips() {
    let (mut app, entity) = fixture();
    let world = app.world_mut();
    world.entity_mut(entity).insert(CameraMotionBlur {
        exposure_seconds: 0.01,
        delta_seconds: 0.01,
        samples: 7,
        reset_epoch: 0,
    });
    world
        .run_system_once(super::prepare::prepare_views)
        .unwrap();
    let target = world.get::<ViewTarget>(entity).unwrap();
    let views = [
        target.main_texture_view().id(),
        target.main_texture_other_view().id(),
    ];
    let depth =
        crate::scene_sampling::view_depth(world.get::<ViewDepthStencilTexture>(entity).unwrap())
            .id();
    let state = world.get::<BlurView>(entity).unwrap();
    for source in views {
        assert!(state.binding(source, depth).is_some());
    }
    target.post_process_write();
    assert!(
        state
            .binding(target.main_texture_view().id(), depth)
            .is_some()
    );
}

#[test]
fn motion_blur_repeated_specialization_reuses_pipelines_without_allocating() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.run_system_once(super::pipeline::init).unwrap();
    world.resource_scope(|world, cache: Mut<PipelineCache>| {
        let mut pipeline = world.resource_mut::<BlurPipeline>();
        for format in [
            render_scene::SCENE_COLOR_FORMAT,
            render_scene::SCENE_HDR_FORMAT,
        ] {
            for samples in [1, 2, 4, 8] {
                let original = pipeline.specialize(&cache, format, samples);
                let footprint = pipeline.footprint(&cache);
                let resolve = pipeline.resolve(&cache, samples);
                let allocated = test_alloc::thread_allocations();
                let repeated = pipeline.specialize(&cache, format, samples);
                assert_eq!(pipeline.footprint(&cache), footprint);
                assert_eq!(pipeline.resolve(&cache, samples), resolve);
                assert_eq!(test_alloc::thread_allocations(), allocated);
                assert_eq!(repeated, original);
            }
        }
    });
}

#[test]
fn motion_blur_warmup_queues_the_same_formats_and_samples_used_for_drawing() {
    use crate::pipeline_warmup::{PrewarmPipelines, WarmView};
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.run_system_once(super::pipeline::init).unwrap();
    world.resource_scope(|world, mut cache: Mut<PipelineCache>| {
        let mut pipeline = world.resource_mut::<BlurPipeline>();
        let mut warmed = Vec::with_capacity(4);
        for hdr in [false, true] {
            for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
                warmed.clear();
                pipeline
                    .prewarm(
                        &cache,
                        WarmView {
                            hdr,
                            msaa,
                            enhanced: false,
                            shader_pack: false,
                            output: None,
                        },
                        &mut warmed,
                    )
                    .unwrap();
                assert_eq!(warmed.len(), 4);
                assert_eq!(warmed[1..3], pipeline.footprint(&cache));
                assert_eq!(warmed[3], pipeline.resolve(&cache, msaa.samples()));
                let tile = crate::queue_review_support::queued_descriptor(&mut cache, warmed[2]);
                assert_eq!(tile.multisample.count, 1);
                assert_eq!(
                    tile.fragment.as_ref().unwrap().entry_point.as_deref(),
                    Some("tile_fragment")
                );
                assert_eq!(
                    tile.fragment.as_ref().unwrap().targets[0]
                        .as_ref()
                        .unwrap()
                        .format,
                    super::footprint::FORMAT
                );
                let format = if hdr {
                    render_scene::SCENE_HDR_FORMAT
                } else {
                    render_scene::SCENE_COLOR_FORMAT
                };
                assert_eq!(
                    warmed[0],
                    pipeline.specialize(&cache, format, msaa.samples())
                );
                let descriptor =
                    crate::queue_review_support::queued_descriptor(&mut cache, warmed[0]);
                assert_eq!(descriptor.multisample.count, msaa.samples());
                assert_eq!(
                    descriptor.fragment.as_ref().unwrap().targets[0]
                        .as_ref()
                        .unwrap()
                        .format,
                    format
                );
                assert_eq!(descriptor.depth_stencil, None);
            }
        }
    });
}

#[test]
fn exposure_and_tile_shader_variants_validate_without_a_native_adapter() {
    for multisampled in [false, true] {
        let source = super::shader::source(include_str!("../motion_blur.wgsl"), multisampled);
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[test]
fn actor_tile_reduction_bounds_serial_source_work() {
    let (app, _) = fixture();
    let world = app.world();
    let device = world.resource::<RenderDevice>();
    let cache = world.resource::<PipelineCache>();
    let pipeline = world.resource::<BlurPipeline>();
    let source_size = Extent3d {
        width: 101,
        height: 99,
        depth_or_array_layers: 1,
    };
    let motion = device
        .create_texture(&TextureDescriptor {
            label: Some("actor reduction work fixture"),
            size: source_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: crate::actor_render::motion::MOTION_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let uniform = device.create_buffer(&BufferDescriptor {
        label: Some("actor reduction exposure fixture"),
        size: std::mem::size_of::<super::history::ExposureUniform>() as u64,
        usage: BufferUsages::UNIFORM,
        mapped_at_creation: false,
    });
    let mut footprint = None;
    super::footprint::Footprint::prepare(
        &mut footprint,
        device,
        cache,
        pipeline.footprint_layouts(),
        &uniform,
        &motion,
        source_size,
    );
    let footprint = footprint.unwrap();
    let source = UVec2::new(source_size.width, source_size.height);
    let rows = UVec2::new(footprint.rows.width(), footprint.rows.height());
    let output = UVec2::new(footprint.texture.width(), footprint.texture.height());
    for (source, output) in [(source, rows), (rows, output)] {
        let serial_reads = source.x.div_ceil(output.x) * source.y.div_ceil(output.y);
        assert!(
            serial_reads <= super::shader::TILE_SIZE,
            "{serial_reads} source texels per reduction invocation"
        );
    }
    assert_eq!(footprint.rows.format(), super::footprint::ROW_FORMAT);
}

#[test]
fn exposure_taps_do_not_repeat_raw_surface_resolves() {
    for multisampled in [false, true] {
        let source = super::shader::source(include_str!("../motion_blur.wgsl"), multisampled);
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let index = module
            .entry_points
            .iter()
            .position(|entry| entry.name == "fragment")
            .unwrap();
        for (handle, global) in module.global_variables.iter() {
            if global
                .binding
                .as_ref()
                .is_some_and(|slot| slot.group == 0 && [2, 4].contains(&slot.binding))
            {
                assert!(
                    info.get_entry_point(index)[handle].is_empty(),
                    "exposure taps must use resolved surfaces, not {:?}",
                    global.name
                );
            }
        }
    }
}

/// Out-of-memory exposure targets turn blur off for the view instead of quitting, and the same
/// extent is not retried every frame.
#[test]
fn exposure_targets_out_of_memory_disable_blur_until_resize() {
    let (mut app, entity) = fixture();
    let world = app.world_mut();
    world.entity_mut(entity).insert(CameraMotionBlur {
        exposure_seconds: 0.01,
        delta_seconds: 0.01,
        samples: 7,
        reset_epoch: 0,
    });
    let mut prepare = IntoSystem::into_system(super::prepare::prepare_views);
    prepare.initialize(world);
    {
        let _failure =
            crate::gpu_allocation::inject(|label, _| label == "resolved exposure surface");
        prepare.run((), world).unwrap();
    }
    let state = world.get::<BlurView>(entity).unwrap();
    assert!(!state.active);
    assert!(state.resolved.is_none() && state.footprint.is_none());
    assert!(!super::applies(world, entity));

    let attempts = std::rc::Rc::new(std::cell::Cell::new(0));
    {
        let attempts = attempts.clone();
        let _observe = crate::gpu_allocation::inject(move |label, _| {
            attempts.set(attempts.get() + usize::from(label == "resolved exposure surface"));
            false
        });
        prepare.run((), world).unwrap();
    }
    assert_eq!(attempts.get(), 0);
    assert!(world.get::<BlurView>(entity).unwrap().resolved.is_none());
}
