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
    let native = render_scene::AtmosphereFrame::default();
    assert!(!needs_native_scene(
        Some(&native),
        None,
        None,
        None,
        None,
        |_| true
    ));
}

#[test]
fn depthless_native_layers_keep_the_scene_copy_path() {
    let mut overlays = render_scene::ScreenOverlayScene::default();
    let mut nametags = render_scene::NametagSceneResource::default();
    assert!(!needs_native_scene(
        None,
        None,
        None,
        Some(&overlays),
        Some(&nametags),
        |_| true
    ));
    overlays.layers.push(render_scene::ScreenOverlayLayer {
        kind: render_scene::ScreenOverlayKind::SpyglassScope,
        rgb: [1.0; 3],
        alpha: 1.0,
    });
    assert!(needs_native_scene(
        None,
        None,
        None,
        Some(&overlays),
        None,
        |_| true
    ));
    nametags
        .0
        .records
        .push(render_model::NametagRecord::default());
    assert!(needs_native_scene(
        None,
        None,
        None,
        None,
        Some(&nametags),
        |_| true
    ));
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
fn unresolved_native_layers_keep_the_scene_copy_path() {
    assert!(!needs_native_scene(None, None, None, None, None, |_| false));
    assert!(needs_native_scene(
        Some(&render_scene::AtmosphereFrame::default()),
        None,
        None,
        None,
        None,
        |_| false
    ));
    for additive in [false, true] {
        let mut particles = render_scene::ParticleGpuFrame::default();
        let instances = std::sync::Arc::from([particles::ParticleInstance::default()]);
        if additive {
            particles.add = instances;
        } else {
            particles.blend = instances;
        }
        assert!(needs_native_scene(
            None,
            Some(&particles),
            None,
            None,
            None,
            |_| false
        ));
    }
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
