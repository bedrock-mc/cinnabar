//! Checks shader-pack camera admission and native queue replacement.

use super::*;
use bevy::render::view::Msaa;

#[test]
fn shaderpack_render_schedule_initializes_without_a_selected_pack() {
    let mut render_app = bevy::app::SubApp::new();
    let (device, _) = wgpu::Device::noop(&Default::default());
    render_app.insert_resource(bevy::render::render_resource::PipelineCache::new(
        RenderDevice::from(device),
        true,
    ));
    render_app.add_schedule(Render::base_schedule());
    let mut app = App::new();
    app.insert_sub_app(RenderApp, render_app);
    finish(&mut app);
    assert!(
        !app.sub_app(RenderApp)
            .world()
            .contains_resource::<ActorSkinPipelines>()
    );
    app.sub_app_mut(RenderApp)
        .world_mut()
        .schedule_scope(Render, |world, schedule| {
            schedule
                .initialize(world)
                .expect("shader-pack render schedule must initialize");
        });
}

#[test]
fn particle_streams_keep_independent_replacement_coverage() {
    let coverage = effect_coverage(
        |effect| effect == Effect::ParticlesTranslucent || effect == Effect::ParticlesAdditive,
        |_| false,
    );
    assert!(!coverage.particles_opaque);
    assert!(coverage.particles_translucent && coverage.particles_additive);
}

#[test]
fn scene_copy_inputs_retain_the_native_far_plane() {
    let inputs = ShaderPackFrameInputs {
        render_distance: MIN_PACK_FAR * 8.0,
        ..default()
    };
    let horizon = MIN_PACK_FAR * 2.0;
    assert_eq!(
        pack_inputs(&inputs, Some(horizon), false).render_distance,
        inputs.render_distance
    );
    assert_eq!(
        pack_inputs(&inputs, Some(horizon), true).render_distance,
        horizon
    );
    assert_eq!(
        pack_inputs(&inputs, None, true).render_distance,
        inputs.render_distance
    );
}

#[test]
fn replacement_is_published_before_native_queue_consumers() {
    #[derive(Resource, Default)]
    struct QueuedTerrain(bool);
    /// Captures the replacement decision observed by a native queue.
    fn native_queue(drawn: Res<ShaderPackDrawnCategories>, mut queued: ResMut<QueuedTerrain>) {
        queued.0 = drawn.terrain;
    }
    let mut world = World::new();
    world.init_resource::<ShaderPackViews>();
    world.init_resource::<ShaderPackEffects>();
    world.insert_resource(ShaderPackDrawnCategories {
        terrain: true,
        ..Default::default()
    });
    world.init_resource::<QueuedTerrain>();
    let mut schedule = bevy::ecs::schedule::Schedule::default();
    schedule.configure_sets(
        (
            RenderSystems::PrepareViews,
            RenderSystems::Queue,
            RenderSystems::PrepareResources,
        )
            .chain(),
    );
    schedule.add_systems((
        crate::chunk::shaderpack_sources::before_native_terrain_queues(configure_replacement),
        native_queue.in_set(RenderSystems::Queue),
    ));
    schedule.run(&mut world);
    assert!(!world.resource::<QueuedTerrain>().0);
}

#[test]
fn opted_in_cameras_admit_single_sample_depth_sampling() {
    let mut app = App::new();
    app.init_resource::<Assets<bevy::shader::Shader>>();
    build(&mut app);
    let camera = app
        .world_mut()
        .spawn((Camera3d::default(), Msaa::Sample4, ShaderPackRendering))
        .id();
    app.update();
    assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
    assert!(
        wgpu::TextureUsages::from(
            app.world()
                .get::<Camera3d>(camera)
                .unwrap()
                .depth_texture_usages
        )
        .contains(wgpu::TextureUsages::TEXTURE_BINDING)
    );
    *app.world_mut().get_mut::<Msaa>(camera).unwrap() = Msaa::Sample8;
    app.update();
    assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
}

#[test]
fn changing_the_option_file_starts_a_new_load() {
    let mut request = ShaderPackRequest {
        pack: Some(PathBuf::from("Outer/Inner")),
        options: Some(PathBuf::from("Outer/Inner.txt")),
        ..default()
    };
    let command_line = loader_key(&request, 0);
    request.options = Some(PathBuf::from("Outer.txt"));
    assert_ne!(loader_key(&request, 0), command_line);
}

/// An idle renderer must not need a GPU or pipeline cache to prepare optional kernels.
#[test]
fn unused_shader_support_never_initializes_gpu_work() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = World::new();
    world.init_resource::<LoadedShaderPack>();
    world.init_resource::<ShaderpackGeometrySettings>();
    world.run_system_once(prepare_actor_pipelines).unwrap();
    world
        .run_system_once(super::super::geometry::prepare_geometry_resources)
        .unwrap();
    assert!(!world.contains_resource::<ActorSkinPipelines>());
    assert!(!world.contains_resource::<ShaderpackGeometry>());
    world.spawn(ShaderPackRendering);
    world.run_system_once(prepare_actor_pipelines).unwrap();
    assert!(!world.contains_resource::<ActorSkinPipelines>());
}

#[test]
fn superseded_loads_retire_before_another_worker_can_start() {
    use bevy::ecs::system::RunSystemOnce;
    let first_path = PathBuf::from("Dusk.zip");
    let mut world = World::new();
    world.insert_resource(ShaderPackRequest {
        pack: Some("Lumen.zip".into()),
        ..default()
    });
    world.init_resource::<ShaderPackFrameInputs>();
    world.init_resource::<ShaderPackSettledRevision>();
    world.init_resource::<LoadedShaderPack>();
    let mut progress = ShaderPackLoadProgress::default();
    let first = progress.start();
    world.insert_resource(progress);
    world.insert_resource(ShaderPackStatus::Loading(first_path.clone()));
    let (send, receive) = mpsc::channel();
    world.insert_resource(Loader {
        key: Some((first_path.clone(), None, 0, 0)),
        job: Some(Mutex::new(receive)),
        ..default()
    });
    world.run_system_once(drive_loader).unwrap();
    assert!(first.is_cancelled());
    assert!(world.resource::<Loader>().retiring);
    assert_eq!(
        world.resource::<Loader>().key.as_ref().unwrap().0,
        first_path
    );
    // Turning off unloads immediately, while the retired receiver still owns its one job.
    world.resource_mut::<ShaderPackRequest>().pack = None;
    world.run_system_once(drive_loader).unwrap();
    assert!(matches!(
        *world.resource::<ShaderPackStatus>(),
        ShaderPackStatus::Idle
    ));
    assert!(world.resource::<Loader>().job.is_some());
    send.send(Err("cancelled".into())).unwrap();
    world.run_system_once(drive_loader).unwrap();
    assert!(world.resource::<Loader>().job.is_none());
    assert!(world.resource::<Loader>().key.is_none());
    assert!(!world.resource::<Loader>().retiring);
    assert!(matches!(
        *world.resource::<ShaderPackStatus>(),
        ShaderPackStatus::Idle
    ));
}
