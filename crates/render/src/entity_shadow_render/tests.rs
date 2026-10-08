use super::*;
use bevy::{
    ecs::system::RunSystemOnce,
    render::renderer::{RenderAdapter, WgpuWrapper},
};
use std::{
    future::Future,
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

fn noop_world() -> World {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..Default::default()
    });
    let mut context = Context::from_waker(Waker::noop());
    let Poll::Ready(Ok(adapter)) =
        pin!(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).poll(&mut context)
    else {
        panic!("noop adapter must be immediate");
    };
    let Poll::Ready(Ok((device, queue))) =
        pin!(adapter.request_device(&wgpu::DeviceDescriptor::default())).poll(&mut context)
    else {
        panic!("noop device must be immediate");
    };
    let device = RenderDevice::from(device);
    let adapter = RenderAdapter(Arc::new(WgpuWrapper::new(adapter)));
    let mut world = World::new();
    world.insert_resource(EntityShadowGpu::new(&device));
    world.insert_resource(PipelineCache::new(device.clone(), adapter.clone(), true));
    world.insert_resource(device);
    world.insert_resource(adapter);
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    world.init_resource::<EntityShadowScene>();
    world.insert_resource(AtmosphereFrame::default());
    world
}

fn publish(world: &mut World, casters: &[EntityShadow]) {
    world.resource_mut::<EntityShadowScene>().0.publish(casters);
}

fn caster(x: f32) -> EntityShadow {
    EntityShadow {
        feet: [x, 64.0, 0.0],
        radius: 0.6,
    }
}

/// A frame whose casters and sky did not change uploads and allocates nothing.
#[test]
fn unchanged_frames_upload_and_allocate_nothing() {
    let mut world = noop_world();
    publish(&mut world, &[caster(0.0), caster(2.0)]);
    let mut system = IntoSystem::into_system(prepare_shadow_buffers);
    system.initialize(&mut world);
    system.run((), &mut world).unwrap();
    let first = world.resource::<EntityShadowGpu>().uploads;
    assert_eq!(first, 2, "casters and parameters upload once");
    let before = crate::alloc_count::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations() - before, 0);
    assert_eq!(world.resource::<EntityShadowGpu>().uploads, first);

    publish(&mut world, &[caster(1.0), caster(2.0)]);
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<EntityShadowGpu>().uploads, first + 1);
    assert_eq!(world.resource::<EntityShadowGpu>().count, 2);
}

#[test]
fn the_instance_buffer_grows_to_fit_and_an_empty_frame_draws_nothing() {
    let mut world = noop_world();
    let many: Vec<_> = (0..100).map(|index| caster(index as f32)).collect();
    publish(&mut world, &many);
    world.run_system_once(prepare_shadow_buffers).unwrap();
    let gpu = world.resource::<EntityShadowGpu>();
    assert_eq!((gpu.count, gpu.capacity), (100, 128));
    publish(&mut world, &[]);
    world.run_system_once(prepare_shadow_buffers).unwrap();
    assert_eq!(world.resource::<EntityShadowGpu>().count, 0);
}

/// Every supported count uses per-sample depth and an overlap stencil, with no colour input.
#[test]
fn pipeline_multiplies_colour_once_per_sample_and_keeps_alpha() {
    let gpu = EntityShadowGpu::new(&noop_world().resource::<RenderDevice>().clone());
    for format in [TextureFormat::Rgba8Unorm, ViewTarget::TEXTURE_FORMAT_HDR] {
        for samples in [1, 2, 4, 8] {
            let descriptor = pipeline_descriptor(
                gpu.layouts[usize::from(samples > 1)].clone(),
                format,
                samples,
            );
            let target = descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap();
            let blend = target.blend.unwrap();
            assert_eq!(blend.color.src_factor, BlendFactor::Zero);
            assert_eq!(blend.color.dst_factor, BlendFactor::Src);
            assert_eq!(target.write_mask, ColorWrites::COLOR);
            assert_eq!(descriptor.multisample.count, samples);
            assert_eq!(descriptor.primitive.cull_mode, Some(Face::Front));
            let stencil = descriptor.depth_stencil.unwrap();
            assert_eq!(stencil.format, TextureFormat::Stencil8);
            assert_eq!(stencil.stencil.back.compare, CompareFunction::NotEqual);
            assert_eq!(stencil.stencil.back.pass_op, StencilOperation::Replace);
        }
    }
}

#[path = "coverage_tests.rs"]
mod coverage;

#[test]
fn shader_parameter_block_matches_the_rust_layout() {
    let source =
        crate::shader_source::standalone(include_str!("../entity_shadow.wgsl"), &["MULTISAMPLED"]);
    let module = naga::front::wgsl::parse_str(&source).expect("entity shadow shader parses");
    let mut layouter = naga::proc::Layouter::default();
    layouter.update(module.to_ctx()).unwrap();
    let params = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some("ShadowParams"))
        .map(|(handle, _)| handle)
        .expect("ShadowParams struct");
    assert_eq!(
        layouter[params].size as usize,
        size_of::<EntityShadowParams>()
    );
}

#[test]
fn shadow_depth_variants_validate_without_resolving_scene_colour() {
    for definitions in [&[][..], &["MULTISAMPLED"][..]] {
        let source =
            crate::shader_source::standalone(include_str!("../entity_shadow.wgsl"), definitions);
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[test]
fn neutral_sky_shades_with_the_plain_grey_and_the_end_has_no_glow() {
    let noon = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
    let colour = shadow_params(&noon).colour;
    for channel in &colour[..3] {
        assert!((0.67..=0.73).contains(channel));
    }
    let end = noon.with_sky_kind(SkyKind::End);
    assert_eq!(shadow_params(&end).colour, [0.7, 0.7, 0.7, 1.0]);
}
