//! Offline terrain-model coverage using an explicitly supplied terrain-model carrier.
use std::{fs, path::PathBuf};

use bevy::math::Vec3;
use {assets::NetworkIdMode, gpu_snapshot::Draw, gpu_snapshot::Gpu};

use crate::{chunk_constants, gpu_snapshot, material_shader, shader_source};

const LIGHTMAP_BINDING: u32 = material_shader::BIOME_QUERY_TABLES_BINDING + 1;

/// Reads the active registry and a requested original-art capture carrier.
pub(crate) fn fixture(
    variable: &str,
) -> Option<(assets::RuntimeAssets, Vec<assets::RegistryRecord>)> {
    let Some(path) = std::env::var_os(variable) else {
        eprintln!("skipping terrain model snapshot: missing {variable} fixture");
        return None;
    };
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let records = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        target["wire_protocol"].as_u64().unwrap() as u32,
    )
    .unwrap()
    .into_vec();
    Some((
        assets::RuntimeAssets::decode(&fs::read(path).unwrap()).unwrap(),
        records,
    ))
}

/// Meshes a rod alongside its supporting cube, retaining the actual visibility and light streams.
pub(crate) fn mesh(
    assets: &assets::RuntimeAssets,
    air: u32,
    stone: u32,
    rod: u32,
    mounting_offset: [i32; 3],
) -> meshing::ChunkMesh {
    let key = world::SubChunkKey::new(0, 0, 0, 0);
    let mut store = world::ChunkStore::new();
    store.mark_sub_chunk_loaded(key).unwrap();
    store
        .update_block(key, world::BlockUpdate::new(1, 1, 1, 0, rod), air)
        .unwrap();
    let support = mounting_offset.map(|offset| (1 + offset) as u8);
    store
        .update_block(
            key,
            world::BlockUpdate::new(support[0], support[1], support[2], 0, stone),
            air,
        )
        .unwrap();
    meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(air),
        assets,
        NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &store.sub_chunk(key).unwrap(),
    )
}

/// Serializes model templates in the renderer's packed storage layout.
fn templates(assets: &assets::RuntimeAssets) -> Vec<u32> {
    let mut words = vec![assets.model_templates().len() as u32];
    for template in assets.model_templates() {
        words.extend([template.quad_start, template.quad_count, template.flags]);
    }
    for quad in assets.model_quads() {
        let positions = quad.positions.into_iter().flatten().collect::<Vec<_>>();
        words.extend(
            positions
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u32::from(p[0] as u16) | (u32::from(p[1] as u16) << 16)),
        );
        let uvs = quad.uvs.into_iter().flatten().collect::<Vec<_>>();
        words.extend(
            uvs.as_chunks::<2>()
                .0
                .iter()
                .map(|p| u32::from(p[0]) | (u32::from(p[1]) << 16)),
        );
        words.extend([quad.material, quad.flags]);
    }
    words
}

/// Uploads the original fixture's real texture-array page for production fragment sampling.
fn atlas(gpu: &Gpu, assets: &assets::RuntimeAssets) -> wgpu::TextureView {
    assert_eq!(
        assets.texture_pages().len(),
        1,
        "bounded original-art fixture"
    );
    let page = &assets.texture_pages()[0].texture;
    let mip = &page.mips[0];
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("original terrain-model capture artwork"),
        size: wgpu::Extent3d {
            width: mip.size,
            height: mip.size,
            depth_or_array_layers: page.layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &mip.rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(mip.size * 4),
            rows_per_image: Some(mip.size),
        },
        texture.size(),
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// Renders both terrain model layers through their production vertex and fragment code.
pub(crate) fn render(
    gpu: &Gpu,
    assets: &assets::RuntimeAssets,
    mesh: &meshing::ChunkMesh,
    mounting_offset: [i32; 3],
    include_rod: bool,
) -> Vec<u8> {
    let mut eye_offset = Vec3::new(1.9, 1.3, 2.5);
    for (axis, mounting_offset) in mounting_offset.into_iter().enumerate() {
        if mounting_offset != 0 {
            eye_offset[axis] *= -(mounting_offset as f32);
        }
    }
    render_at(
        gpu,
        assets,
        mesh,
        Vec3::splat(1.5) + eye_offset,
        Vec3::splat(1.5),
        include_rod,
    )
}

/// Renders terrain model and cube streams with an explicit account-free fixture camera.
pub(crate) fn render_at(
    gpu: &Gpu,
    assets: &assets::RuntimeAssets,
    mesh: &meshing::ChunkMesh,
    eye: Vec3,
    center: Vec3,
    include_models: bool,
) -> Vec<u8> {
    let storage = wgpu::BufferUsages::STORAGE;
    let uniform = wgpu::BufferUsages::UNIFORM;
    let draw_refs = mesh
        .model_draw_refs()
        .iter()
        .chain(mesh.transparent_model_draw_refs());
    let draw_count = draw_refs.clone().count();
    let refs_start = (draw_count * 2).next_multiple_of(4);
    let light_start = refs_start + mesh.model_refs().len() * 4;
    let mut streams = draw_refs
        .flat_map(|r| {
            let mut words = r.words();
            words[0] += refs_start as u32 / 4;
            words
        })
        .collect::<Vec<_>>();
    streams.resize(refs_start, 0);
    for reference in mesh.model_refs() {
        let mut words = reference.words();
        words[2] += light_start as u32 / 2;
        streams.extend(words);
    }
    streams.extend(mesh.model_lighting().iter().flat_map(|light| {
        let [a, b, c, d] = light.samples();
        [
            u32::from(a) | (u32::from(b) << 16),
            u32::from(c) | (u32::from(d) << 16),
        ]
    }));
    let geometry = gpu.words(&streams, storage);
    let cube_quads = gpu.words(
        &mesh
            .cube_quads()
            .iter()
            .flat_map(|q| q.words())
            .collect::<Vec<_>>(),
        storage,
    );
    let cube_lights = gpu.words(
        &mesh
            .cube_lighting()
            .iter()
            .flat_map(|light| {
                let [a, b, c, d] = light.samples();
                [
                    u32::from(a) | (u32::from(b) << 16),
                    u32::from(c) | (u32::from(d) << 16),
                ]
            })
            .collect::<Vec<_>>(),
        storage,
    );
    let asset_tables = gpu.words(
        &gpu_snapshot::chunk_asset_table_words(&[], &[], &templates(assets)),
        storage,
    );
    let origins = gpu.words(&[0; 8], storage);
    let materials = gpu.words(
        &assets
            .materials()
            .iter()
            .flat_map(|m| {
                let size = assets.texture_source_size(m.texture);
                let grid = assets.texture_source_grid(m.texture);
                [
                    material_shader::gpu_grid_texture_ref(
                        m.texture,
                        size.map(|v| v << grid),
                        assets.texture_pages()[0].texture.mips[0].size,
                        grid,
                    ),
                    m.flags,
                    m.animation,
                    m.variation_start,
                    m.variation_count,
                    m.variation_weight,
                ]
            })
            .collect::<Vec<_>>(),
        storage,
    );
    let camera = glam::camera::rh::view::look_at_mat4(eye, center, Vec3::Y);
    let projection =
        glam::camera::rh::proj::directx::orthographic(-1.35, 1.35, -1.35, 1.35, 10.0, 0.1);
    let view = gpu.buffer(&gpu_snapshot::view(projection * camera, eye), uniform);
    let empty = gpu.words(&[0; 64], storage);
    let clock = gpu.words(&[0; 4], uniform);
    let lightmap = gpu.lightmap(bytemuck::cast_slice(
        &render_model::LightmapInputs::default().build(),
    ));
    let query = gpu.words(&meshing::biome_lattice::query_table_words(), uniform);
    let mut atmosphere = [0.0; 36];
    atmosphere[19] = 100.0;
    atmosphere[20] = 200.0;
    let atmosphere = gpu.buffer(&atmosphere, uniform);
    let atlas = atlas(gpu, assets);
    let sampler = gpu.device.create_sampler(&Default::default());
    let mut bindings = [
        (0, view.as_entire_binding()),
        (2, origins.as_entire_binding()),
        (3, materials.as_entire_binding()),
        (
            material_shader::NATIVE_LEAF_SAMPLER_BINDING,
            wgpu::BindingResource::Sampler(&sampler),
        ),
        (6, wgpu::BindingResource::Sampler(&sampler)),
        (7, empty.as_entire_binding()),
        (8, empty.as_entire_binding()),
        (9, asset_tables.as_entire_binding()),
        (11, clock.as_entire_binding()),
        (13, geometry.as_entire_binding()),
        (15, atmosphere.as_entire_binding()),
        (
            material_shader::BIOME_QUERY_TABLES_BINDING,
            query.as_entire_binding(),
        ),
        (LIGHTMAP_BINDING, lightmap.as_entire_binding()),
    ]
    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource })
    .to_vec();
    bindings.extend(
        material_shader::NATIVE_LEAF_TEXTURE_BINDINGS.map(|binding| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(&atlas),
        }),
    );
    let mut cube_bindings = bindings
        .iter()
        .filter(|entry| entry.binding != 12 && entry.binding != 13)
        .cloned()
        .collect::<Vec<_>>();
    cube_bindings.extend([
        wgpu::BindGroupEntry {
            binding: 1,
            resource: cube_quads.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 13,
            resource: cube_lights.as_entire_binding(),
        },
    ]);
    let source = format!(
        "{}\n{}",
        shader_source::standalone(include_str!("../../../src/model.wesl"), &[]).replace(
            "@group(1) @binding(0)",
            &format!("@group(0) @binding({LIGHTMAP_BINDING})")
        ),
        VERTEX.replace(
            "INDICES",
            &chunk_constants::STATIC_QUAD_INDICES
                .map(|i| format!("{i}u"))
                .join(", ")
        )
    );
    let cube_source = format!(
        "{}\n{}",
        shader_source::standalone(include_str!("../../../src/chunk.wesl"), &[]).replace(
            "@group(1) @binding(0)",
            &format!("@group(0) @binding({LIGHTMAP_BINDING})")
        ),
        CUBE_VERTEX.replace(
            "INDICES",
            &chunk_constants::STATIC_QUAD_INDICES
                .map(|i| format!("{i}u"))
                .join(", ")
        )
    );
    let opaque_end = if include_models {
        mesh.model_draw_refs().len() as u32 * 6
    } else {
        0
    };
    let model_end = if include_models {
        draw_count as u32 * 6
    } else {
        0
    };
    gpu.render_sources_srgb(
        &[&cube_source, &source, &source],
        &[
            Draw {
                fragment: "fragment_solid",
                vertices: 0..mesh.cube_quads().len() as u32 * 6,
                bindings: &cube_bindings,
                blend: None,
                write_depth: true,
            },
            Draw {
                fragment: "fragment",
                vertices: 0..opaque_end,
                bindings: &bindings,
                blend: None,
                write_depth: true,
            },
            Draw {
                fragment: "fragment_blend",
                vertices: opaque_end..model_end,
                bindings: &bindings,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_depth: false,
            },
        ],
        &["support_vertex", "rod_vertex", "rod_vertex"].map(|vertex| gpu_snapshot::DrawPipeline {
            vertex,
            topology: wgpu::PrimitiveTopology::TriangleList,
        }),
    )
}

const VERTEX: &str = r#"
@vertex fn rod_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let indices = array<u32,6>(INDICES);
    return model_vertex(indices[index % 6u], index / 6u);
}
"#;

const CUBE_VERTEX: &str = r#"
@vertex fn support_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let indices = array<u32,6>(INDICES);
    return cube_vertex(indices[index % 6u], index / 6u);
}
"#;

#[test]
fn rod_snapshots_render_cube_and_model_draws_with_all_texture_pages_bound() {
    let Some((assets, records)) = fixture("CINNABAR_END_ROD_CARRIER") else {
        return;
    };
    let Some(gpu) = Gpu::for_fixture("rod snapshot texture page bindings") else {
        return;
    };
    let id = |name: &str| {
        records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let mounting_offset = [0, -1, 0];
    let mesh = mesh(
        &assets,
        id("minecraft:air"),
        id("minecraft:stone"),
        id("minecraft:end_rod"),
        mounting_offset,
    );
    let cubes = render(&gpu, &assets, &mesh, mounting_offset, false);
    let models = render(&gpu, &assets, &mesh, mounting_offset, true);
    assert!(!mesh.model_draw_refs().is_empty());
    assert_ne!(
        cubes, models,
        "the rod must contribute visible model pixels"
    );
}
