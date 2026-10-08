//! Execute production model vertices with packed offsets and ordinary quarter turns.
use crate::{
    gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE},
    shader_source,
};

fn source() -> String {
    let source = shader_source::standalone(include_str!("../../src/model.wgsl"), &["ENHANCED"])
        .replace("@vertex\nfn vertex(", "fn model_vertex(")
        .replace(
            "@builtin(vertex_index) vertex_index: u32",
            "vertex_index: u32",
        )
        .replace(
            "@builtin(instance_index) instance_index: u32",
            "instance_index: u32",
        )
        .replace("@group(1) @binding(0)", "@group(0) @binding(30)");
    let source = (0..7).fold(source, |source, binding| {
        source.replace(
            &format!("@group(2) @binding({binding})"),
            &format!("@group(0) @binding({})", binding + 20),
        )
    });
    format!("{source}\n{WITNESS}")
}

#[test]
fn bamboo_offsets_do_not_rotate_enhanced_surface_normals() {
    let Some(gpu) = Gpu::for_fixture("bamboo Enhanced vertex normals") else {
        return;
    };
    let source = source();
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bamboo Enhanced vertex normals"),
            source: wgpu::ShaderSource::Wgsl(source.clone().into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("witness"),
            compilation_options: Default::default(),
            cache: None,
        });
    let storage = wgpu::BufferUsages::STORAGE;
    let uniform = wgpu::BufferUsages::UNIFORM;
    let view = gpu.buffer(&[0.0; 104], uniform);
    let origins = gpu.words(&[0; 8], storage);
    let materials = gpu.words(&[0, 0, assets::NO_ANIMATION, 0, 0, 0], storage);
    let animations = gpu.words(&[0; 5], storage);
    let frames = gpu.words(&[0], storage);
    let clock = gpu.words(&[0; 4], uniform);
    let templates = gpu.words(&template_words(), storage);
    let geometry = gpu.words(&geometry_words(), storage);
    let frame = gpu.buffer(&[0.0; 256], uniform);
    let lightmap = gpu.buffer(
        bytemuck::cast_slice(&render::LightmapInputs::default().build()),
        uniform,
    );
    let classes = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let bytes = (8 * 2 * size_of::<[f32; 4]>()) as u64;
    let output = gpu.words(
        &vec![0; bytes as usize / 4],
        storage | wgpu::BufferUsages::COPY_SRC,
    );
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bindings = [
        (0, view.as_entire_binding()),
        (2, origins.as_entire_binding()),
        (3, materials.as_entire_binding()),
        (9, animations.as_entire_binding()),
        (10, frames.as_entire_binding()),
        (11, clock.as_entire_binding()),
        (12, templates.as_entire_binding()),
        (13, geometry.as_entire_binding()),
        (20, frame.as_entire_binding()),
        (23, wgpu::BindingResource::TextureView(&classes)),
        (31, output.as_entire_binding()),
        (30, lightmap.as_entire_binding()),
    ]
    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &bindings,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(8, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
    gpu.queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let values: Vec<[f32; 4]> =
        bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();
    if let Some(path) = std::env::var_os("CINNABAR_BAMBOO_SHADER_CAPTURE") {
        let bindings: Vec<_> = bindings
            .into_iter()
            .filter(|binding| binding.binding != 31)
            .collect();
        let pixels = gpu.render(
            &source,
            "normal_vertex",
            &[Draw {
                fragment: "normal_fragment",
                vertices: 0..48,
                bindings: &bindings,
                blend: None,
                write_depth: false,
            }],
        );
        image::RgbaImage::from_raw(SNAPSHOT_SIDE, SNAPSHOT_SIDE, pixels)
            .unwrap()
            .save(path)
            .unwrap();
    }
    for case in 0..8 {
        let expected = if case < 4 {
            [-1.0, 0.0, 0.0]
        } else {
            [
                [-1.0, 0.0, 0.0],
                [0.0, 0.0, -1.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
            ][case - 4]
        };
        assert_eq!(
            &values[case * 2][..3],
            &expected,
            "case {case}: offset must not become rotation"
        );
        assert_eq!(values[case * 2][3], 1.0, "fixture vertex must be visible");
        assert_eq!(
            values[case * 2 + 1][3],
            if case < 4 { 1.0 } else { 0.0 },
            "only admitted bamboo selects bounded tile sampling"
        );
        if case < 4 {
            let x = world::bamboo::OFFSET_MIN
                + case as f32 * world::bamboo::OFFSET_SPAN
                    / (world::bamboo::OFFSET_STEPS - 1) as f32
                + 0.5;
            assert!((values[case * 2 + 1][0] - x).abs() < 1.0e-6);
        }
    }
}

fn template_words() -> Vec<u32> {
    let mut words = vec![2, 0, 1, assets::MODEL_TEMPLATE_FLAG_BAMBOO, 0, 1, 0];
    let positions: [i16; 12] = [128, 0, 128, 128, 0, 176, 128, 256, 176, 128, 256, 128];
    words.extend(
        positions
            .chunks_exact(2)
            .map(|p| u32::from(p[0] as u16) | u32::from(p[1] as u16) << 16),
    );
    words.extend([0; 4]);
    words.extend([0, assets::BlockFace::West.model_quad_face_id()]);
    words
}

fn geometry_words() -> Vec<u32> {
    let mut words: Vec<u32> = (0..8)
        .flat_map(|case| meshing::PackedModelDrawRef::new(4 + case, 0).words())
        .collect();
    for case in 0..8 {
        let variant = if case < 4 { case | (4 << 4) } else { case - 4 };
        words.extend(
            meshing::PackedModelRef::new(variant << 12, u32::from(case >= 4), 24 + case, 1).words(),
        );
    }
    words.extend([0xffff_ffff; 16]);
    words
}

const WITNESS: &str = r#"
@group(0) @binding(31) var<storage, read_write> results: array<vec4<f32>>;
@compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) id: vec3<u32>) {
    let vertex = model_vertex(0u, id.x);
    results[id.x * 2u] = vec4(vertex.normal, f32(vertex.visible & MODEL_VISIBLE));
    results[id.x * 2u + 1u] = vec4(vertex.world_position, f32((vertex.visible & MODEL_BOUNDED_TILE) != 0u));
}
@vertex fn normal_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array(vec2(0.0,0.0),vec2(1.0,0.0),vec2(1.0,1.0),vec2(0.0,1.0));
    let indices = array(0u,1u,2u,0u,2u,3u);
    let case_index = index / 6u;
    var out = model_vertex(0u, case_index);
    let point = (vec2(f32(case_index),0.0) + corners[indices[index % 6u]]) / vec2(8.0,1.0);
    out.clip_position = vec4(point * vec2(2.0,-2.0) + vec2(-1.0,1.0),0.5,1.0);
    return out;
}
@fragment fn normal_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(in.normal * 0.5 + vec3(0.5),1.0);
}
"#;
