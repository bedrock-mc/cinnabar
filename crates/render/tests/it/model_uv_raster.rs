//! Position-dependent rotation must reach decoded world-model vertices.
use crate::{chunk_constants, gpu_snapshot, shader_source};
use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu};
use meshing::{PackedModelDrawRef, PackedModelRef};

const COLUMNS: usize = 8;
const CASES: usize = 32;

/// Encodes one top, bottom or side quad with a distinct authored UV rectangle.
fn templates() -> Vec<u32> {
    let mut words = vec![CASES as u32];
    for index in 0..CASES {
        words.extend([index as u32, 1, 0]);
    }
    for index in 0..CASES {
        let positions: [i16; 12] = [0, 240, 0, 0, 240, 256, 256, 240, 256, 256, 240, 0];
        words.extend(
            positions
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u32::from(p[0] as u16) | (u32::from(p[1] as u16) << 16)),
        );
        let uvs: [u16; 8] = [512, 1024, 512, 3584, 3072, 3584, 3072, 1024];
        words.extend(
            uvs.as_chunks::<2>()
                .0
                .iter()
                .map(|p| u32::from(p[0]) | (u32::from(p[1]) << 16)),
        );
        words.extend([index as u32, [2, 1, 6, 2][index / COLUMNS]]);
    }
    words
}

/// Calculates the independently verified face turn from wrapping signed coordinates.
fn turn(position: [i32; 3]) -> u32 {
    let [x, y, z] = position.map(|value| value as u32);
    let h = z.wrapping_mul(0x06ebfff5) ^ x.wrapping_mul(0x002fc20f) ^ y;
    let code = h.wrapping_mul(0x0285b825).wrapping_add(11).wrapping_mul(h) >> 24 & 3;
    [0, 1, 3, 2][code as usize]
}

#[test]
fn world_model_uvs_rotate_top_and_bottom_by_position_and_preserve_side_and_static_uvs() {
    let Some(gpu) = Gpu::for_fixture("world model UV rotation") else {
        return;
    };
    let storage = wgpu::BufferUsages::STORAGE;
    let uniform = wgpu::BufferUsages::UNIFORM;
    let positions: [[i32; 3]; CASES] =
        std::array::from_fn(|index| [-17 + (index % COLUMNS) as i32, 69, -124]);
    let flags: [u32; CASES] = std::array::from_fn(|index| match index / COLUMNS {
        0 | 1 => assets::MATERIAL_FLAG_ISOTROPIC,
        2 => 0,
        _ => assets::MATERIAL_FLAG_ISOTROPIC | 1,
    });
    let mut seen = [false; 4];
    let mut expected = Vec::new();
    for (index, position) in positions.iter().enumerate() {
        let rotation = match index / COLUMNS {
            0 | 1 => turn(*position),
            2 => 0,
            _ => 1,
        };
        if index < COLUMNS {
            seen[rotation as usize] = true;
        }
        for [u, v] in [[0.125, 0.25], [0.125, 0.875], [0.75, 0.875], [0.75, 0.25]] {
            expected.extend(match rotation {
                1 => [v, 1.0 - u],
                2 => [1.0 - u, 1.0 - v],
                3 => [1.0 - v, u],
                _ => [u, v],
            });
            expected.extend([0.0, 0.0]);
        }
    }
    assert!(seen.into_iter().all(|value| value));
    let origins = gpu.words(
        &positions
            .into_iter()
            .flat_map(|p| [p[0] as u32, p[1] as u32, p[2] as u32, 0, 0, 0, 0, 0])
            .collect::<Vec<_>>(),
        storage,
    );
    let materials = gpu.words(
        &flags
            .into_iter()
            .flat_map(|flags| [0, flags, assets::NO_ANIMATION, 0, 0, 0])
            .collect::<Vec<_>>(),
        storage,
    );
    let ref_word = CASES * 2;
    let light_word = ref_word + CASES * 4;
    let mut geometry = (0..CASES)
        .flat_map(|index| PackedModelDrawRef::new((ref_word / 4 + index) as u32, 0).words())
        .collect::<Vec<_>>();
    for index in 0..CASES {
        geometry.extend(
            PackedModelRef::new(0, index as u32, (light_word / 2 + index) as u32, 1).words(),
        );
    }
    geometry.extend(crate::solid_terrain_raster::lighting_words(CASES));
    let geometry = gpu.words(&geometry, storage);
    let templates = gpu.words(&templates(), storage);
    let view = gpu.buffer(&gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO), uniform);
    let empty = gpu.words(&[0; 64], storage);
    let clock = gpu.words(&[0; 4], uniform);
    let lightmap = gpu.buffer(
        bytemuck::cast_slice(&render::LightmapInputs::default().build()),
        uniform,
    );
    let expected = gpu.buffer(&expected, uniform);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("original asymmetric UV witness"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let texels = (0..16)
        .flat_map(|index| [17 + index * 13, 240 - index * 11, 30 + index * 7, 255])
        .collect::<Vec<u8>>();
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(16),
            rows_per_image: Some(4),
        },
        texture.size(),
    );
    let atlas = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&Default::default());
    let query_tables = gpu.words(
        &meshing::biome_lattice::query_table_words(),
        wgpu::BufferUsages::UNIFORM,
    );
    let bindings = [
        (0, view.as_entire_binding()),
        (2, origins.as_entire_binding()),
        (3, materials.as_entire_binding()),
        (4, wgpu::BindingResource::TextureView(&atlas)),
        (6, wgpu::BindingResource::Sampler(&sampler)),
        (7, empty.as_entire_binding()),
        (8, empty.as_entire_binding()),
        (9, empty.as_entire_binding()),
        (10, empty.as_entire_binding()),
        (11, clock.as_entire_binding()),
        (12, templates.as_entire_binding()),
        (13, geometry.as_entire_binding()),
        (
            crate::material_shader::BIOME_QUERY_TABLES_BINDING,
            query_tables.as_entire_binding(),
        ),
        (20, lightmap.as_entire_binding()),
        (21, expected.as_entire_binding()),
    ]
    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
    let source = shader_source::standalone(include_str!("../../src/model.wesl"), &[])
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)");
    let witness = WITNESS.replace(
        "INDICES",
        &chunk_constants::STATIC_QUAD_INDICES
            .map(|i| format!("{i}u"))
            .join(", "),
    );
    let candidate = format!("{source}\n{}", witness.replace("REFERENCE_UV", ""));
    let reference = format!(
        "{source}\n{}",
        witness.replace(
            "REFERENCE_UV",
            "out.uv = expected_uvs.values[cell*4u+corner].xy;"
        )
    );
    let draws = [Draw {
        fragment: "uv_fragment",
        vertices: 0..CASES as u32 * 6,
        bindings: &bindings,
        blend: None,
        write_depth: true,
    }];
    let expected = gpu.render(&reference, "uv_vertex", &draws);
    let actual = gpu.render(&candidate, "uv_vertex", &draws);
    let mismatches = actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        mismatches, 0,
        "decoded model vertices must sample the independently rotated authored UV rectangle"
    );
}

const WITNESS: &str = r#"
struct ExpectedUvs { values: array<vec4<f32>, 128>, }
@group(0) @binding(21) var<uniform> expected_uvs: ExpectedUvs;
@vertex fn uv_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let indices = array<u32, 6>(INDICES);
    let corners = array(vec2(0.0,0.0), vec2(1.0,0.0), vec2(1.0,1.0), vec2(0.0,1.0));
    let cell = index / 6u;
    let corner = indices[index % 6u];
    var out = model_vertex(cell*4u+corner,cell);
    REFERENCE_UV
    let p = (vec2(f32(cell%8u),f32(cell/8u))+corners[corner])/vec2(8.0,4.0);
    out.clip_position = vec4(p*vec2(2.0,-2.0)+vec2(-1.0,1.0),0.5+expected_uvs.values[0].z,1.0);
    return out;
}
@fragment fn uv_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(block_textures_page_0,block_sampler,in.uv,0);
}
"#;
