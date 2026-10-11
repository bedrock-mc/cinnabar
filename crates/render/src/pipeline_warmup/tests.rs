use super::*;
use bevy::{
    asset::uuid_handle,
    render::{
        render_resource::{
            ColorTargetState, ColorWrites, FragmentState, RenderPipelineDescriptor, TextureFormat,
            VertexState,
        },
        renderer::RenderDevice,
    },
};

const UNLOADED_SHADER: Handle<Shader> = uuid_handle!("5b0e7c3d-8a41-4f2e-9d6b-1c7a2e9f4b80");

/// Validation-only backend with synchronous compilation, so `process_queue` finishes each pipeline.
fn cache() -> PipelineCache {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions {
                enable: true,
                ..Default::default()
            },
            ..default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&default())).unwrap();
    let (device, _) = bevy::tasks::block_on(adapter.request_device(&default())).unwrap();

    let mut cache = PipelineCache::new(RenderDevice::from(device), true);
    cache.set_shader(
        Handle::<Shader>::default().id(),
        crate::shader_safety::from_wgsl(
            "@vertex fn vertex() -> @builtin(position) vec4f { return vec4f(0.0); }\n\
             @fragment fn fragment() -> @location(0) vec4f { return vec4f(1.0); }",
            "warmup-test.wgsl",
        ),
    );
    cache
}

fn descriptor(shader: Handle<Shader>, write_mask: ColorWrites) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask,
            })],
            ..default()
        }),
        ..default()
    }
}

#[derive(Resource)]
struct TwoModes {
    shader: Handle<Shader>,
    calls: usize,
}

impl PrewarmPipelines for TwoModes {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        _view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError> {
        self.calls += 1;
        for mask in [ColorWrites::RED, ColorWrites::GREEN] {
            ids.push(cache.queue_render_pipeline(descriptor(self.shader.clone(), mask)));
        }
        Ok(())
    }
}

const VIEW: WarmView = WarmView {
    msaa: Msaa::Off,
    hdr: false,
    enhanced: false,
    output: None,
};

fn world(shader: Handle<Shader>) -> World {
    let mut world = World::new();
    world.insert_resource(cache());
    world.insert_resource(TwoModes { shader, calls: 0 });
    world.insert_resource(WarmupRegistry {
        views: vec![VIEW],
        ..default()
    });
    world
}

fn ready(world: &mut World) -> bool {
    world.resource_scope(|world, cache: Mut<PipelineCache>| {
        registered_pipelines_ready(&cache, &mut world.resource_mut::<WarmupRegistry>())
    })
}

#[test]
fn loading_waits_for_every_queued_variant_and_steady_frames_do_not_rewarm() {
    let mut world = world(Handle::default());
    let warm = world.register_system(prewarm_owner::<TwoModes>);
    world.run_system(warm).unwrap();
    assert_eq!(world.resource::<WarmupRegistry>().ids.len(), 2);
    assert!(!ready(&mut world), "queued pipelines are not usable yet");
    world.resource_mut::<PipelineCache>().process_queue();
    assert!(ready(&mut world));
    for _ in 0..3 {
        world.run_system(warm).unwrap();
    }
    assert_eq!(world.resource::<TwoModes>().calls, 1);
    assert_eq!(world.resource::<WarmupRegistry>().ids.len(), 2);
}

#[test]
fn a_new_view_configuration_warms_again_and_closes_the_gate() {
    let mut world = world(Handle::default());
    let warm = world.register_system(prewarm_owner::<TwoModes>);
    world.run_system(warm).unwrap();
    world.resource_mut::<PipelineCache>().process_queue();
    assert!(ready(&mut world));
    world.resource_mut::<WarmupRegistry>().views.push(WarmView {
        msaa: Msaa::Sample4,
        ..VIEW
    });
    world.run_system(warm).unwrap();
    assert_eq!(world.resource::<TwoModes>().calls, 2);
    assert!(!ready(&mut world));
}

#[test]
fn unloaded_shaders_hold_loading() {
    let mut world = world(UNLOADED_SHADER);
    let warm = world.register_system(prewarm_owner::<TwoModes>);
    world.run_system(warm).unwrap();
    for _ in 0..2 {
        world.resource_mut::<PipelineCache>().process_queue();
        assert!(!ready(&mut world));
    }
}

#[test]
fn no_view_never_releases_loading() {
    let cache = cache();
    assert!(!registered_pipelines_ready(
        &cache,
        &mut WarmupRegistry::default()
    ));
    assert!(!PipelineWarmupReadiness::default().is_ready());
}
