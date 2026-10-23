//! Trusted helper kernels owned only while a shader renderer needs them.

use super::{geometry::ShaderpackGeometrySettings, plugin::LoadedShaderPack};
use bevy::{
    prelude::*,
    render::{render_resource::PipelineCache, renderer::RenderDevice},
};
use render_scene::shaderpack::ShaderPackRendering;

/// Derived once from complete helper layouts so native frames do not allocate layout entries.
static REQUIRED_STORAGE_BUFFERS: std::sync::LazyLock<u32> = std::sync::LazyLock::new(|| {
    super::actors::required_storage_buffers().max(super::geometry::required_storage_buffers())
});

/// Rejects devices whose per-stage storage budget cannot run the optional helper kernels.
pub(super) fn validate_device(limits: &wgpu::Limits) -> Result<(), String> {
    let required = *REQUIRED_STORAGE_BUFFERS;
    let available = limits.max_storage_buffers_per_shader_stage;
    if available < required {
        return Err(format!(
            "Shader packs require {required} storage buffers per shader stage; this GPU provides {available}. Native rendering remains available."
        ));
    }
    Ok(())
}

/// Keeps optional shaders, compiled pipelines and their layouts out of the native renderer's cache.
#[derive(Resource)]
pub(super) struct Kernels {
    pub cache: PipelineCache,
    shared_loaded: bool,
}

impl Kernels {
    /// Registers trusted helper sources in an empty cache after the first shader request.
    pub(super) fn new(device: RenderDevice, synchronous: bool) -> Self {
        let mut cache = PipelineCache::new(device, synchronous);
        super::actors::load_shaders(&mut cache);
        super::geometry::load_shaders(&mut cache);
        super::atlas::load_shaders(&mut cache);
        Self {
            cache,
            shared_loaded: false,
        }
    }
}

/// Supplies native modules and later asset changes only while an optional cache exists.
pub(super) fn extract_shared_shaders(
    kernels: Option<ResMut<Kernels>>,
    shaders: bevy::render::Extract<Res<Assets<Shader>>>,
    mut events: bevy::render::Extract<MessageReader<AssetEvent<Shader>>>,
) {
    let Some(mut kernels) = kernels else {
        events.clear();
        return;
    };
    if !kernels.shared_loaded {
        for (id, shader) in shaders.iter() {
            kernels.cache.set_shader(id, shader.clone());
        }
        kernels.shared_loaded = true;
        events.clear();
        return;
    }
    for event in events.read() {
        match event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                if let Some(shader) = shaders.get(*id) {
                    kernels.cache.set_shader(*id, shader.clone());
                }
            }
            AssetEvent::Removed { id } => kernels.cache.remove_shader(*id),
            AssetEvent::Unused { .. } | AssetEvent::LoadedWithDependencies { .. } => {}
        }
    }
}

/// Creates and advances optional pipelines while requested, then releases their whole cache.
pub(super) fn prepare_kernels(
    mut commands: Commands,
    device: Res<RenderDevice>,
    pack: Option<Res<LoadedShaderPack>>,
    cameras: Query<(), With<ShaderPackRendering>>,
    geometry: Option<Res<ShaderpackGeometrySettings>>,
    kernels: Option<ResMut<Kernels>>,
) {
    let requested = pack.map_or_else(
        || geometry.is_some_and(|settings| settings.enabled),
        |pack| pack.0.is_some() && !cameras.is_empty(),
    ) && device.limits().max_storage_buffers_per_shader_stage
        >= *REQUIRED_STORAGE_BUFFERS;
    match (requested, kernels) {
        (true, Some(mut kernels)) => kernels.cache.process_queue(),
        (true, None) => {
            commands.insert_resource(Kernels::new(device.clone(), false));
        }
        (false, Some(_)) => {
            commands.remove_resource::<Kernels>();
        }
        (false, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// Checks complete pipeline budgets, including output groups, at the device boundary.
    #[test]
    fn optional_helpers_require_their_complete_pipeline_storage_budget() {
        let required = super::super::actors::required_storage_buffers()
            .max(super::super::geometry::required_storage_buffers());
        assert!(required > 4);
        for available in [4, required - 1, required] {
            let result = validate_device(&wgpu::Limits {
                max_storage_buffers_per_shader_stage: available,
                ..default()
            });
            assert_eq!(result.is_ok(), available >= required);
        }
    }

    #[test]
    fn optional_kernel_cache_follows_the_renderer_without_replacing_the_native_cache() {
        let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits {
                max_storage_buffers_per_shader_stage: *REQUIRED_STORAGE_BUFFERS,
                ..default()
            },
            ..default()
        });
        let device = RenderDevice::from(device);
        let mut world = World::new();
        world.insert_resource(device.clone());
        bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
        let mut native = PipelineCache::new(device, true);
        let shader: Handle<Shader> =
            bevy::asset::uuid_handle!("ea93f5d7-b05b-4626-9776-faf1012a33b4");
        native.set_shader(
            shader.id(),
            crate::shader_safety::from_wgsl(
                "@compute @workgroup_size(1) fn main() {}",
                "native_fixture.wgsl",
            ),
        );
        let native_pipeline = native.queue_compute_pipeline(
            bevy::render::render_resource::ComputePipelineDescriptor {
                shader,
                entry_point: Some("main".into()),
                ..default()
            },
        );
        native.process_queue();
        assert!(native.get_compute_pipeline(native_pipeline).is_some());
        world.insert_resource(native);
        world.init_resource::<ShaderpackGeometrySettings>();
        world.run_system_once(prepare_kernels).unwrap();
        assert!(!world.contains_resource::<Kernels>());
        world.resource_mut::<ShaderpackGeometrySettings>().enabled = true;
        world.run_system_once(prepare_kernels).unwrap();
        assert!(world.contains_resource::<Kernels>());
        assert!(
            world
                .resource::<PipelineCache>()
                .get_compute_pipeline(native_pipeline)
                .is_some()
        );
        // An explicit Off wins over geometry settings still extracted from the previous frame.
        world.init_resource::<LoadedShaderPack>();
        world.run_system_once(prepare_kernels).unwrap();
        assert!(!world.contains_resource::<Kernels>());
        assert!(
            world
                .resource::<PipelineCache>()
                .get_compute_pipeline(native_pipeline)
                .is_some()
        );
    }
}
