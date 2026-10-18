//! Validate shared world fixture resources without requiring a GPU adapter.

use crate::{gpu_snapshot, shader_source};
use gpu_snapshot::Gpu;

#[test]
fn shared_atmosphere_and_cloud_words_match_shader_uniform_sizes() {
    for (shader, uniform, bytes) in [
        (
            include_str!("../../../src/atmosphere.wesl"),
            "WorldAtmosphere",
            bytemuck::cast_slice::<f32, u8>(&gpu_snapshot::atmosphere()).len(),
        ),
        (
            include_str!("../../../src/cloud.wesl"),
            "NativeCloudUniform",
            bytemuck::bytes_of(&gpu_snapshot::cloud_uniform()).len(),
        ),
    ] {
        let source = shader_source::standalone(shader, &[]);
        let module = naga::front::wgsl::parse_str(&source).expect("fixture shader parses");
        let ty = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some(uniform))
            .expect("production uniform exists")
            .1;
        let naga::TypeInner::Struct { span, .. } = ty.inner else {
            panic!("production uniform must be a struct");
        };
        assert_eq!(span as usize, bytes, "{uniform} fixture size");
    }
}

#[test]
fn shared_lighting_constructor_covers_world_fragment_resources() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let gpu = Gpu {
        device,
        queue,
        backend: wgpu::Backend::Noop,
    };
    let lightmap = gpu.lightmap(&[1.0; 256 * 4]);
    let lighting = gpu_snapshot::WorldLighting::new(&gpu, &lightmap, None);
    let resources = lighting
        .flat_bindings()
        .map(|mut entry| {
            entry.binding -= gpu_snapshot::FIXTURE_LIGHTING_BASE;
            entry
        })
        .collect::<Vec<_>>();
    for shader in [
        include_str!("../../../src/chunk.wesl"),
        include_str!("../../../src/model.wesl"),
        include_str!("../../../src/liquid.wesl"),
        include_str!("../../../src/transparent_terrain.wesl"),
    ] {
        for definitions in [&[][..], &["NATIVE_GAMMA_BLEND"][..]] {
            let source = shader_source::composed(shader, definitions);
            let module = naga::front::wgsl::parse_str(&source).expect("world fixture parses");
            let entries = module
                .entry_points
                .iter()
                .filter(|entry| entry.stage == naga::ShaderStage::Fragment)
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>();
            gpu_snapshot::bindings_for(&source, &entries, 1, &resources);
            shader_source::assert_single_bind_group(
                &shader_source::bind_lighting(&source),
                &entries,
            );
        }
    }
}
