//! Deterministic resource and allocation contracts for the optional exposure pass.

use super::{CameraMotionBlur, pipeline::BlurPipeline, prepare::BlurView};
use bevy::{
    camera::{
        CameraMainTextureUsages, CameraOutputMode, ClearColorConfig, MsaaWriteback,
        NormalizedRenderTarget, RenderTarget,
    },
    core_pipeline::core_3d::graph::Core3d,
    ecs::system::RunSystemOnce,
    prelude::*,
    render::{
        camera::ExtractedCamera,
        render_graph::RenderSubGraph,
        render_resource::*,
        renderer::RenderDevice,
        texture::{CachedTexture, OutputColorAttachment, TextureCache},
        view::{ViewDepthTexture, ViewTarget, ViewTargetAttachments, prepare_view_targets},
    },
};

fn fixture() -> (App, Entity) {
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
            render_graph: Core3d.intern(),
            order: 0,
            output_mode: CameraOutputMode::default(),
            msaa_writeback: MsaaWriteback::default(),
            clear_color: ClearColorConfig::default(),
            sorted_camera_index_for_target: 0,
            exposure: 1.0,
            hdr: false,
        },
        ViewDepthTexture::new(
            CachedTexture {
                texture: depth,
                default_view: depth_view,
            },
            Some(0.0),
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
    let allocated = crate::alloc_count::thread_allocations();
    prepare.run((), world).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations(), allocated);
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
    let depth = world.get::<ViewDepthTexture>(entity).unwrap().view().id();
    let binding = world
        .get::<BlurView>(entity)
        .unwrap()
        .binding(target, depth)
        .unwrap()
        .id();
    prepare.run((), world).unwrap();
    let allocated = crate::alloc_count::thread_allocations();
    prepare.run((), world).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations(), allocated);
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
    let allocated = crate::alloc_count::thread_allocations();
    prepare.run((), world).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations(), allocated);
    assert!(!super::applies(world, entity));
}

#[test]
fn motion_blur_repeated_specialization_reuses_pipelines_without_allocating() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.run_system_once(super::pipeline::init).unwrap();
    world.resource_scope(|world, cache: Mut<PipelineCache>| {
        let mut pipeline = world.resource_mut::<BlurPipeline>();
        for format in [
            TextureFormat::bevy_default(),
            ViewTarget::TEXTURE_FORMAT_HDR,
        ] {
            for samples in [1, 2, 4, 8] {
                let original = pipeline.specialize(&cache, format, samples);
                let allocated = crate::alloc_count::thread_allocations();
                let repeated = pipeline.specialize(&cache, format, samples);
                assert_eq!(crate::alloc_count::thread_allocations(), allocated);
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
        let mut warmed = Vec::with_capacity(1);
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
                        },
                        &mut warmed,
                    )
                    .unwrap();
                assert_eq!(warmed.len(), 1);
                let format = if hdr {
                    ViewTarget::TEXTURE_FORMAT_HDR
                } else {
                    TextureFormat::bevy_default()
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
