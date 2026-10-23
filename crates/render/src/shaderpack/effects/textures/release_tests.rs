use super::*;
use crate::block_entity::{BlockEntityRenderPlugin, prepare_block_entity_resources};
use crate::particle_render::{ParticleRenderPlugin, prepare_particle_resources};
use crate::shaderpack::{
    loader, passes::ViewState, pipelines::GpuContext, plugin::ShaderPackViews,
};
use bevy::ecs::system::RunSystemOnce;
use bevy::render::RenderApp;

/// Installs both atlas owners so tests exercise their production release paths.
struct AtlasOwners;

impl Plugin for AtlasOwners {
    /// Adds the renderers whose atlases the active pack shares.
    fn build(&self, app: &mut App) {
        app.add_plugins((BlockEntityRenderPlugin, ParticleRenderPlugin));
    }
}

/// Creates a validated headless pack that samples the native effect atlases.
fn app() -> App {
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
    let fragment = "#version 120\nuniform sampler2D gtexture;\nvoid main() { gl_FragColor = texture2D(gtexture, vec2(0.5)); }";
    let source = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
        ("gbuffers_basic.vsh", vertex),
        ("gbuffers_basic.fsh", fragment),
        ("gbuffers_terrain.vsh", vertex),
        ("gbuffers_terrain.fsh", fragment),
    ]));
    let pack = Arc::new(
        loader::build(
            &source,
            "atlas release fixture",
            crate::shaderpack::plugin::JAVA_DIMENSIONS[0],
            &default(),
            None,
            &gpu,
            &default(),
            &default(),
        )
        .unwrap(),
    );
    let mut views = ShaderPackViews::default();
    for index in 0..2 {
        views.states.get_mut().unwrap().insert(
            Entity::from_raw_u32(index).unwrap(),
            ViewState::new(&gpu.device, Arc::clone(&pack), [4; 2]).unwrap(),
        );
    }
    let mut app = crate::render_test_support::owner_app(
        RenderDevice::from(gpu.device),
        gpu.queue,
        AtlasOwners,
    );
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.insert_resource(views);
    world.insert_resource(LoadedShaderPack(Some(pack)));
    world.insert_resource(crate::actor::gpu::ArtworkRawViews(true));
    world.init_resource::<ShaderpackEffectTextures>();
    world.init_resource::<render_scene::BlockSelectionFrameResource>();
    publish(world, 1, false);
    publish(world, 1, true);
    world.run_system_once(prepare_effect_textures).unwrap();
    bind(world);
    app
}

/// Publishes distinct original pixels through the selected native atlas upload system.
fn publish(world: &mut World, value: u8, particles: bool) {
    let side = 16;
    if particles {
        world.insert_resource(render_scene::ParticleGpuFrame {
            base: Some(vec![value; (side * side * 4) as usize].into()),
            atlas_side: side,
            ..default()
        });
        world.run_system_once(prepare_particle_resources).unwrap();
    } else {
        let mut blocks = render_scene::BlockEntityFrameResource::default();
        blocks.atlas = Some(Arc::new(
            render_model::block_entity::BlockEntityAtlasImage {
                identity: [value; 32],
                size: [side; 2],
                static_height: side,
                static_rgba8: vec![value; (side * side * 4) as usize].into(),
            },
        ));
        world.insert_resource(blocks);
        world
            .run_system_once(prepare_block_entity_resources)
            .unwrap();
    }
}

/// Builds both parity bindings over the currently shared textures, as a pack frame does.
fn bind(world: &World) {
    let textures = world.resource::<ShaderpackEffectTextures>();
    let device = world.resource::<RenderDevice>().wgpu_device();
    for state in world
        .resource::<ShaderPackViews>()
        .states
        .lock()
        .unwrap()
        .values_mut()
    {
        let pack = Arc::clone(&state.pack);
        state
            .bindings
            .bind_draws(device, &pack, &mut state.targets, |kind| match kind {
                DrawKind::Effect(effect) => textures.views(effect),
                _ => None,
            });
        state.atlas_generation = Some((None, Some(textures.generation)));
    }
}

/// Checks that released effect draws retain no texture binding in any view or parity.
fn assert_released(world: &World, effects: &[Effect]) {
    let textures = world.resource::<ShaderpackEffectTextures>();
    for &effect in effects {
        match effect {
            Effect::DamagedBlock => assert!(textures.cracks.is_none()),
            _ => assert!(textures.particles.is_none()),
        }
    }
    for state in world
        .resource::<ShaderPackViews>()
        .states
        .lock()
        .unwrap()
        .values()
    {
        assert!(state.atlas_generation.is_none());
        for effect in effects {
            let index = state
                .pack
                .draws
                .iter()
                .position(|d| d.kind == DrawKind::Effect(*effect))
                .unwrap();
            assert!(
                state.bindings.draws[index]
                    .groups
                    .iter()
                    .flatten()
                    .all(Option::is_none)
            );
        }
    }
}

#[test]
fn block_entity_release_drops_shaderpack_views_and_rebinds_the_replacement() {
    let mut app = app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let previous = world
        .resource::<ShaderpackEffectTextures>()
        .cracks
        .as_ref()
        .unwrap()
        .texture;
    let particles = world
        .resource::<ShaderpackEffectTextures>()
        .particles
        .as_ref()
        .unwrap()
        .texture;
    let generation = world.resource::<ShaderpackEffectTextures>().generation;
    {
        let _fault = crate::gpu_allocation::inject(|label, _| label == "block-entity atlas");
        publish(world, 2, false);
    }
    assert_released(world, &[Effect::DamagedBlock]);
    assert_eq!(
        world
            .resource::<ShaderpackEffectTextures>()
            .particles
            .as_ref()
            .unwrap()
            .texture,
        particles
    );
    world.run_system_once(prepare_effect_textures).unwrap();
    assert_released(world, &[Effect::DamagedBlock]);
    publish(world, 3, false);
    world.run_system_once(prepare_effect_textures).unwrap();
    let textures = world.resource::<ShaderpackEffectTextures>();
    assert!(textures.generation > generation);
    assert_eq!(
        textures.cracks.as_ref().unwrap().texture,
        world
            .resource::<BlockEntityGpu>()
            .atlas_texture()
            .unwrap()
            .id()
    );
    assert_ne!(textures.cracks.as_ref().unwrap().texture, previous);
    bind(world);
    assert_bound(world, &[Effect::DamagedBlock]);
}

#[test]
fn particle_release_drops_shaderpack_views_and_rebinds_the_replacement() {
    let mut app = app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let previous = world
        .resource::<ShaderpackEffectTextures>()
        .particles
        .as_ref()
        .unwrap()
        .texture;
    let effects = [
        Effect::Particles,
        Effect::ParticlesTranslucent,
        Effect::ParticlesAdditive,
    ];
    {
        let _fault = crate::gpu_allocation::inject(|label, _| label == "particle atlas");
        publish(world, 2, true);
    }
    assert_released(world, &effects);
    assert_eq!(
        world
            .resource::<ShaderpackEffectTextures>()
            .particle_atlas_size(),
        [0; 2]
    );
    world.run_system_once(prepare_effect_textures).unwrap();
    assert_released(world, &effects);
    publish(world, 3, true);
    world.run_system_once(prepare_effect_textures).unwrap();
    let textures = world.resource::<ShaderpackEffectTextures>();
    assert_eq!(
        textures.particles.as_ref().unwrap().texture,
        world
            .resource::<ParticleGpu>()
            .atlas_texture()
            .unwrap()
            .id()
    );
    assert_ne!(textures.particles.as_ref().unwrap().texture, previous);
    assert_eq!(textures.particle_atlas_size(), [16; 2]);
    bind(world);
    assert_bound(world, &effects);
}

/// Confirms replacement effect draws have rebuilt their sampled-texture bindings.
fn assert_bound(world: &World, effects: &[Effect]) {
    for state in world
        .resource::<ShaderPackViews>()
        .states
        .lock()
        .unwrap()
        .values()
    {
        for effect in effects {
            let index = state
                .pack
                .draws
                .iter()
                .position(|d| d.kind == DrawKind::Effect(*effect))
                .unwrap();
            assert!(
                state.bindings.draws[index]
                    .groups
                    .iter()
                    .all(|groups| groups.iter().any(Option::is_some))
            );
        }
    }
}
