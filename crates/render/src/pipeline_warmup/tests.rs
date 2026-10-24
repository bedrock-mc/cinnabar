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
use render_scene::PipelineWarmupReadiness;

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
        view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError> {
        self.calls += 1;
        for mask in [ColorWrites::RED, ColorWrites::GREEN] {
            let mut descriptor = descriptor(self.shader.clone(), mask);
            descriptor.multisample.count = view.msaa.samples();
            ids.push(cache.queue_render_pipeline(descriptor));
        }
        Ok(())
    }
}

const VIEW: WarmView = WarmView {
    msaa: Msaa::Off,
    hdr: false,
    enhanced: false,
    shader_pack: false,
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
fn loading_prepares_single_sample_coverage_before_msaa_is_disabled() {
    let mut world = world(Handle::default());
    world.resource_mut::<WarmupRegistry>().views[0].msaa = Msaa::Sample4;
    let warm = world.register_system(prewarm_owner::<TwoModes>);
    world.run_system(warm).unwrap();
    assert_eq!(world.resource::<WarmupRegistry>().ids.len(), 4);
    assert!(!ready(&mut world));
    world.resource_mut::<PipelineCache>().process_queue();
    assert!(ready(&mut world));

    world.resource_mut::<WarmupRegistry>().views[0].msaa = Msaa::Off;
    world.run_system(warm).unwrap();
    assert_eq!(world.resource::<WarmupRegistry>().ids.len(), 4);
    assert!(
        ready(&mut world),
        "Performance coverage was already compiled"
    );
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

/// Upscaling's blocking blit wait in `Prepare` starts every queued pipeline, so warm-up must
/// queue after it and still before `Render` starts this frame's queue.
#[test]
fn warmup_queues_after_the_blocking_blit_and_before_render() {
    #[derive(Resource, Default)]
    struct Queued(Vec<usize>);
    fn probe(registry: Res<WarmupRegistry>, mut queued: ResMut<Queued>) {
        queued.0.push(registry.ids.len());
    }
    let mut app = App::new();
    let mut render = SubApp::new();
    render.add_schedule(Render::base_schedule());
    app.insert_sub_app(RenderApp, render);
    register::<TwoModes>(&mut app);
    let render = app.sub_app_mut(RenderApp);
    render
        .insert_resource(cache())
        .insert_resource(TwoModes {
            shader: Handle::default(),
            calls: 0,
        })
        .init_resource::<Queued>()
        .add_systems(
            Render,
            (
                probe.in_set(RenderSystems::Prepare),
                probe.in_set(RenderSystems::Render),
            ),
        );
    render
        .world_mut()
        .spawn((crate::render_test_support::camera(false), Msaa::Off));
    render.world_mut().run_schedule(Render);
    assert_eq!(render.world().resource::<Queued>().0, [0, 2]);
}
