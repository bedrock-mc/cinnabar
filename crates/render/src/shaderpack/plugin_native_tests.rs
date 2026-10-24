//! Native layers must not interrupt a ready pack's replacement queues.

use super::*;
use bevy::ecs::system::RunSystemOnce;

/// Builds an empty, ready pack with compiled actor kernels and no game carriers.
pub(in crate::shaderpack) fn ready_world() -> World {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions {
                enable: true,
                ..default()
            },
            ..default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&default())).unwrap();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
            ..default()
        },
        ..default()
    }))
    .unwrap();
    let gpu = GpuContext {
        device,
        queue,
        adapter,
    };
    let vertex = "#version 120\nvoid main() { gl_Position = ftransform(); }";
    let fragment = "#version 120\nvoid main() { gl_FragColor = vec4(0.25); }";
    let source = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
        ("gbuffers_basic.vsh", vertex),
        ("gbuffers_basic.fsh", fragment),
        ("gbuffers_terrain.vsh", vertex),
        ("gbuffers_terrain.fsh", fragment),
    ]));
    let pack = Arc::new(
        super::super::loader::build(
            &source,
            "fixture",
            JAVA_DIMENSIONS[0],
            &default(),
            None,
            &gpu,
            &default(),
            &default(),
        )
        .unwrap(),
    );
    assert!(!pack.draws.is_empty(), "fixture must supply world programs");
    let state = ViewState::new(&gpu.device, Arc::clone(&pack), [4, 4]).unwrap();
    assert!(state.geometry_targets_ready());
    let mut world = World::new();
    let entity = world.spawn(ShaderPackRendering).id();
    let mut views = ShaderPackViews::default();
    views.states.get_mut().unwrap().insert(entity, state);
    world.insert_resource(views);
    world.insert_resource(LoadedShaderPack(Some(pack)));
    let device = RenderDevice::from(gpu.device);
    let geometry = ShaderpackGeometry::ready_fixture(&device);
    assert!(geometry.supported() && geometry.all_sections_fit() && geometry.origin().is_some());
    world.insert_resource(geometry);
    world.init_resource::<ShaderPackEffects>();
    world.init_resource::<ShaderPackDrawnCategories>();
    prepare_actor_kernels(&mut world, &device);
    world.insert_resource(device);
    world.insert_resource(RenderQueue::new(gpu.queue));
    world
}

/// Compiles the real skinning kernels so each transient test starts from native world readiness.
fn prepare_actor_kernels(world: &mut World, device: &RenderDevice) {
    bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    world.insert_resource(super::super::kernels::Kernels::new(device.clone(), true));
    world.init_resource::<ActorSkinPipelines>();
    test_time::eventually("shader-pack actor fixture kernels", || {
        world
            .resource_mut::<super::super::kernels::Kernels>()
            .cache
            .process_queue();
        world
            .resource::<ActorSkinPipelines>()
            .kernels(&world.resource::<super::super::kernels::Kernels>().cache)
            .is_some()
    });
}

#[test]
fn pack_passes_require_a_loaded_pack() {
    let mut world = ready_world();
    let pack = world.remove_resource::<LoadedShaderPack>().unwrap();
    assert!(!world.run_system_once(pack_loaded).unwrap());
    world.init_resource::<LoadedShaderPack>();
    assert!(!world.run_system_once(pack_loaded).unwrap());
    world.insert_resource(pack);
    assert!(world.run_system_once(pack_loaded).unwrap());
    world.resource_mut::<LoadedShaderPack>().0 = None;
    assert!(!world.run_system_once(pack_loaded).unwrap());
}

/// Runs the production queue decision and checks every replacement category.
pub(super) fn assert_replacement(world: &mut World) {
    world.run_system_once(configure_replacement).unwrap();
    assert_eq!(
        *world.resource::<ShaderPackDrawnCategories>(),
        ShaderPackDrawnCategories {
            terrain: true,
            actors: true,
            block_entities: true,
            hand: true,
        }
    );
}

#[test]
fn pending_actor_kernels_leave_pack_categories_drawn() {
    let mut world = ready_world();
    world.remove_resource::<ActorSkinPipelines>();
    assert_replacement(&mut world);
}

#[test]
fn uncovered_particles_leave_pack_categories_drawn() {
    let mut world = ready_world();
    for mode in 0..3 {
        let mut frame = render_scene::ParticleGpuFrame::default();
        let instances = Arc::from([particles::ParticleInstance::default()]);
        match mode {
            0 => frame.opaque = instances,
            1 => frame.blend = instances,
            _ => frame.add = instances,
        }
        world.insert_resource(frame);
        assert_replacement(&mut world);
    }
}

#[test]
fn particle_activation_uses_one_render_path_per_blend_mode() {
    let mut world = ready_world();
    world.init_resource::<ShaderpackEffectTextures>();
    let instances = Arc::from([particles::ParticleInstance {
        center_light: [0.0, 0.0, -1.0, 0.0],
        axis_x: [0.5, 0.0, 0.0, 0.0],
        axis_y: [0.0, 0.5, 0.0, 0.0],
        color: [1.0; 4],
        uv: [0.0, 0.0, 1.0, 1.0],
        ..default()
    }]);
    world.insert_resource(render_scene::ParticleGpuFrame {
        atlas_side: 1,
        opaque: Arc::clone(&instances),
        blend: Arc::clone(&instances),
        add: instances,
        ..default()
    });
    let view = world
        .query_filtered::<Entity, With<ShaderPackRendering>>()
        .single(&world)
        .unwrap();
    let modes = [
        Effect::Particles,
        Effect::ParticlesTranslucent,
        Effect::ParticlesAdditive,
    ];
    for pack_ready in [false, true] {
        world.run_system_once(configure_replacement).unwrap();
        let native = world
            .run_system_once(
                move |pack: render_scene::shaderpack::ShaderPackDrawnEffects| {
                    [
                        !pack.replaces(view, |drawn| drawn.particles_opaque),
                        !pack.replaces(view, |drawn| drawn.particles_translucent),
                        !pack.replaces(view, |drawn| drawn.particles_additive),
                    ]
                },
            )
            .unwrap();
        world.run_system_once(prepare_effect_textures).unwrap();
        let textures = world.resource::<ShaderpackEffectTextures>();
        assert!(
            modes
                .into_iter()
                .all(|effect| textures.views(effect).is_some())
        );
        let inputs = EffectInputs {
            coverage: *world.resource::<ShaderPackEffects>(),
            camera: EffectCamera {
                position: Vec3::ZERO,
                model_view: Mat4::IDENTITY,
                focal: [1.0; 2],
                screen: [4; 2],
            },
            frame: default(),
            atmosphere: None,
            atmosphere_view: None,
            atmosphere_textures: None,
            clouds_visible: false,
            particles: Some(world.resource::<render_scene::ParticleGpuFrame>()),
            particle_atlas_side: textures.particle_atlas_size()[0] as u32,
            precipitation: None,
            lightning: None,
            selection: None,
            block_entities: None,
            seconds: 0.0,
        };
        let mut buffers = super::super::effects::EffectBuffers::default();
        super::super::effects::prepare_effects(
            world.resource::<RenderDevice>().wgpu_device(),
            world.resource::<RenderQueue>(),
            world.resource::<LoadedShaderPack>().0.as_ref().unwrap(),
            &mut buffers,
            &inputs,
        );
        for (effect, native) in modes.into_iter().zip(native) {
            let pack = buffers.range(effect).is_some();
            assert!(
                !(native && pack),
                "{effect:?} must not use both native and pack paths"
            );
            assert_eq!((native, pack), (!pack_ready, pack_ready), "{effect:?}");
        }
    }
}

#[test]
fn uncovered_weather_leaves_pack_categories_drawn() {
    let mut world = ready_world();
    world.insert_resource(render_scene::PrecipitationScene {
        layers: vec![render_model::PrecipitationLayerRecord {
            particle_count: 1,
            ..default()
        }],
        ..default()
    });
    assert_replacement(&mut world);
}

#[test]
fn screen_overlay_leaves_pack_categories_drawn() {
    let mut world = ready_world();
    world.insert_resource(render_scene::ScreenOverlayScene {
        layers: vec![render_scene::ScreenOverlayLayer {
            kind: render_scene::ScreenOverlayKind::SpyglassScope,
            rgb: [1.0; 3],
            alpha: 1.0,
        }],
        ..default()
    });
    assert_replacement(&mut world);
}

#[test]
fn nametag_records_leave_pack_categories_drawn() {
    let mut world = ready_world();
    world.init_resource::<render_scene::NametagSceneResource>();
    world
        .resource_mut::<render_scene::NametagSceneResource>()
        .records
        .push(render_model::NametagRecord::default());
    assert_replacement(&mut world);
}

#[test]
fn unsupported_actor_leaves_pack_categories_drawn() {
    let mut world = ready_world();
    let mut frame = render_scene::ActorRenderFrame::default();
    frame.rig.instances = Arc::from([render_model::ActorGpuInstance {
        material: render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG,
        ..default()
    }]);
    assert!(!actors::compatibility::frame_supported(
        Some(&frame),
        None,
        None
    ));
    world.insert_resource(frame);
    assert_replacement(&mut world);
}

#[test]
fn uncovered_sky_leaves_pack_categories_drawn() {
    let mut world = ready_world();
    world.init_resource::<render_scene::AtmosphereFrame>();
    assert_replacement(&mut world);
}

#[test]
fn actor_allocation_failure_leaves_pack_terrain_ready() {
    let mut world = ready_world();
    for state in world
        .resource_mut::<ShaderPackViews>()
        .states
        .get_mut()
        .unwrap()
        .values_mut()
    {
        state.actors.allocation_failed = true;
    }
    assert_replacement(&mut world);
}

#[test]
fn missing_geometry_keeps_native_queues_available() {
    let mut world = ready_world();
    world.remove_resource::<ShaderpackGeometry>();
    world.run_system_once(configure_replacement).unwrap();
    assert_eq!(
        *world.resource::<ShaderPackDrawnCategories>(),
        ShaderPackDrawnCategories::default()
    );
}
