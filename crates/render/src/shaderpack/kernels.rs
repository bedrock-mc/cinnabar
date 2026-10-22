//! Trusted helper kernels owned only while a shader renderer needs them.

use super::{geometry::ShaderpackGeometrySettings, plugin::LoadedShaderPack};
use bevy::{
    prelude::*,
    render::{render_resource::PipelineCache, renderer::RenderDevice},
};
use render_scene::shaderpack::ShaderPackRendering;

/// Keeps optional shaders, compiled pipelines and their layouts out of the native renderer's cache.
#[derive(Resource)]
pub(super) struct Kernels {
    pub cache: PipelineCache,
}

impl Kernels {
    /// Registers trusted helper sources in an empty cache after the first shader request.
    pub(super) fn new(device: RenderDevice, synchronous: bool) -> Self {
        let mut cache = PipelineCache::new(device, synchronous);
        super::actors::load_shaders(&mut cache);
        super::geometry::load_shaders(&mut cache);
        super::atlas::load_shaders(&mut cache);
        Self { cache }
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
    );
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

    #[test]
    fn optional_kernel_cache_follows_the_renderer_without_replacing_the_native_cache() {
        let (device, _) = wgpu::Device::noop(&Default::default());
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
