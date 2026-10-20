//! Expansion kernels against an original packed section; the fixture section and its
//! expansion are shared with `expansion_draw_tests`.

use super::jobs::{JobKind, MaterialTables, ModelDrawRefs, SectionGeometry, plan_section};
use super::pipelines::{
    EXPAND_PARAMS_BYTES, EXPAND_WORKGROUP_SIZE, expansion_shader_source, output_layout,
    source_layout,
};
use super::vertex::{
    BLOCK_ID, COLOR, IRIS_VERTEX_WORDS, LIGHTMAP, MID_BLOCK, MID_TEX, NORMAL, POSITION, TANGENT,
    TerrainLayout, UV0,
};
use crate::gpu_snapshot::Gpu;
use crate::shaderpack::atlas;
use assets::Material;
use bevy::math::Vec3;
use meshing::{Face, PackedQuad};
use wgpu::util::DeviceExt;

const COMMON_MODULE: &str = "render::shaderpack::geometry::expand_common";

fn kernel(root: &str) -> String {
    crate::shader_source::composed_prepared_with(
        &expansion_shader_source(root),
        &[],
        vec![(
            COMMON_MODULE,
            expansion_shader_source(include_str!("expand_common.wesl")),
        )],
    )
}

#[test]
fn every_expansion_kernel_composes_from_the_packed_decoders_and_validates() {
    for root in [
        include_str!("expand_cubes.wesl"),
        include_str!("expand_models.wesl"),
        include_str!("expand_liquids.wesl"),
    ] {
        let source = kernel(root);
        let module = naga::front::wgsl::parse_str(&source).expect("composed kernel parses");
        for entry in &module.entry_points {
            assert_eq!(entry.workgroup_size, [EXPAND_WORKGROUP_SIZE, 1, 1]);
        }
        assert!(
            !source.contains("@vertex"),
            "imported draw entry points are pruned"
        );
    }
}

#[test]
fn production_devices_bind_every_kernels_storage_buffers() {
    // The client raises the device limit to what the packed chunk layout needs; the kernels
    // must fit within it.
    for kind in super::jobs::JOB_KINDS {
        assert!(
            super::pipelines::storage_buffers_per_stage(kind)
                <= crate::required_vertex_storage_buffers(),
            "{kind:?}"
        );
    }
}

// --- Fixture section --------------------------------------------------------------------------

pub(super) const SECTION_ORIGIN: [i32; 3] = [32, 64, -16];
pub(super) const ANCHOR: [i32; 3] = [16, 64, -16];
pub(super) const ATLAS_SIZE: [u32; 2] = [256, 128];
/// Sprite of page 0 layer 3 in the fixture atlas: x, y, width, height.
pub(super) const GRASS_RECT: [u32; 4] = [32, 16, 16, 16];
/// Sprite of page 1 layer 2.
const LEAF_RECT: [u32; 4] = [64, 0, 32, 32];
const GRASS: u32 = 1;
const LEAF: u32 = 2;
pub(super) const GRASS_BLOCK_ID: i32 = 42;
const GRASS_EMISSION: u32 = 7;
pub(super) const SPRITE_RGBA: [u8; 4] = [200, 100, 50, 255];

/// Light samples: block in bits 0-3, sky in 4-7, AO level in 8-10.
const fn light(block: u16, sky: u16, ao: u16) -> u16 {
    block | (sky << 4) | (ao << 8)
}

const TOP_LIGHT: [u16; 4] = [
    light(3, 15, 0),
    light(5, 15, 2),
    light(7, 14, 0),
    light(1, 13, 1),
];
const SIDE_LIGHT: [u16; 4] = [light(15, 0, 0); 4];

fn materials() -> Vec<Material> {
    let material = |texture: u32, flags| Material {
        texture: assets::TextureRef::from_raw(texture).unwrap(),
        flags,
        ..Material::unvaried()
    };
    vec![
        Material::unvaried(),
        material(3, 0),
        material((1 << 28) | 2, assets::MATERIAL_FLAG_TWO_SIDED),
    ]
}

fn cube_quads() -> [PackedQuad; 2] {
    [
        // A greedy 2x1 top face over blocks (2,5,3) and (3,5,3).
        PackedQuad::new([2, 5, 3], Face::PositiveY, 2, 1, GRASS),
        // A two-sided west face, outside the solid run.
        PackedQuad::new([6, 2, 1], Face::NegativeX, 1, 1, LEAF),
    ]
}

pub(super) struct Expanded {
    pub(super) gpu: Gpu,
    pub(super) vertices: wgpu::Buffer,
    /// The stream's layout.
    pub(super) layout: TerrainLayout,
    pub(super) words: Vec<u32>,
    pub(super) faces: u32,
    pub(super) categories: [std::ops::Range<u32>; 3],
}

fn words_of(samples: [u16; 4]) -> [u32; 2] {
    [
        u32::from(samples[0]) | (u32::from(samples[1]) << 16),
        u32::from(samples[2]) | (u32::from(samples[3]) << 16),
    ]
}

fn read_buffer(gpu: &Gpu, buffer: &wgpu::Buffer, bytes: u64) -> Vec<u8> {
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &readback, 0, bytes);
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    readback
        .slice(..)
        .get_mapped_range()
        .expect("readback buffer is mapped")
        .to_vec()
}

fn layout(
    gpu: &Gpu,
    descriptor: &bevy::render::render_resource::BindGroupLayoutDescriptor,
) -> wgpu::BindGroupLayout {
    gpu.device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &descriptor.entries,
        })
}

/// One section's records for one kernel, and the geometry stream (binding 13) it reads.
struct Fixture<'a> {
    kind: JobKind,
    cube_quads: &'a [PackedQuad],
    liquid_quads: &'a [meshing::PackedLiquidQuad],
    streams: Vec<u32>,
    /// The origin's stream bases: liquid records at `z`, their lighting records at `w`.
    stream_bases: [u32; 4],
    /// The vertex layout the kernel writes.
    layout: TerrainLayout,
}

/// Expands the fixture section's cube quads with the production kernel and layouts, in the
/// wide stream.
pub(super) fn expand_fixture(name: &str) -> Option<Expanded> {
    expand_fixture_in(name, TerrainLayout::WIDE)
}

/// Expands the fixture section's cube quads into a stream of `layout`.
pub(super) fn expand_fixture_in(name: &str, layout: TerrainLayout) -> Option<Expanded> {
    let quads = cube_quads();
    let mut streams = words_of(TOP_LIGHT).to_vec();
    streams.extend(words_of(SIDE_LIGHT));
    expand_section(
        name,
        Fixture {
            kind: JobKind::Cube,
            cube_quads: &quads,
            liquid_quads: &[],
            streams,
            stream_bases: [0; 4],
            layout,
        },
    )
}

/// Expands `fixture` with the production kernel of its kind and layouts.
fn expand_section(name: &str, fixture: Fixture<'_>) -> Option<Expanded> {
    let kind = fixture.kind;
    let limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: super::pipelines::storage_buffers_per_stage(kind),
        ..wgpu::Limits::default()
    };
    let gpu = Gpu::for_fixture_with_limits(name, wgpu::Features::empty(), limits)?;
    let materials = materials();
    let quads = fixture.cube_quads;
    let tables = MaterialTables {
        materials: &materials,
        model_templates: &[],
        model_quads: &[],
    };
    let section = SectionGeometry {
        metadata_index: 0,
        quad_base: 0,
        cube_quads: quads,
        solid_len: 1,
        model_refs: &[],
        model_draw_base: 0,
        model_draw_refs: &[],
        transparent_model_draw_base: 0,
        transparent_model_draw_refs: ModelDrawRefs::Local(&[]),
        liquid_base: 0,
        liquid_quads: fixture.liquid_quads,
    };
    let plan = plan_section(&section, &tables);
    let storage = |words: &[u32]| {
        gpu.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(words),
                usage: wgpu::BufferUsages::STORAGE,
            })
    };
    let mut quad_words: Vec<u32> = quads.iter().flat_map(PackedQuad::words).collect();
    quad_words.resize(quad_words.len().max(2), 0);
    let mut origin_words: Vec<u32> = SECTION_ORIGIN.map(|axis| axis as u32).to_vec();
    origin_words.push(0);
    origin_words.extend(fixture.stream_bases);
    let material_words: Vec<u32> = materials
        .iter()
        .flat_map(|material| {
            [
                material.texture.raw(),
                material.flags,
                material.animation,
                material.variation_start,
                material.variation_count,
                material.variation_weight,
            ]
        })
        .collect();
    let stream_words = fixture.streams;
    let uniform = |words: &[u32]| {
        gpu.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(words),
                usage: wgpu::BufferUsages::UNIFORM,
            })
    };
    let animation_clock = uniform(&[0; 4]);
    let biome_query_tables = uniform(&meshing::biome_lattice::query_table_words());
    let mut attributes: Vec<u32> = [(-1_i32) as u32, 0].repeat(materials.len());
    attributes[GRASS as usize * 2] = GRASS_BLOCK_ID as u32;
    attributes[GRASS as usize * 2 + 1] = GRASS_EMISSION;
    let mut job_words = attributes.clone();
    job_words.extend(plan.jobs[kind as usize].iter().flatten());
    let mut remap = vec![[0_u32; 2]; atlas::static_sprite_slot(1, 2) as usize + 1];
    remap[0] = ATLAS_SIZE;
    let rect = |[x, y, w, h]: [u32; 4]| [x | (y << 16), w | (h << 16)];
    remap[atlas::static_sprite_slot(0, 3) as usize] = rect(GRASS_RECT);
    remap[atlas::static_sprite_slot(1, 2) as usize] = rect(LEAF_RECT);
    let remap_words: Vec<u32> = remap.into_iter().flatten().collect();
    let mut params = [0_u32; (EXPAND_PARAMS_BYTES / 4) as usize];
    params[..3].copy_from_slice(&ANCHOR.map(|axis| axis as u32));
    params[6] = attributes.len() as u32;
    params[7] = materials.len() as u32;
    params[8..12].copy_from_slice(&fixture.layout.expand_words());
    params[12] = plan.jobs[kind as usize].len() as u32;
    let params = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let vertex_bytes = u64::from(plan.faces) * 4 * fixture.layout.stride();
    let vertices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: vertex_bytes,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::VERTEX
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let (root, entry) = match kind {
        JobKind::Cube => (include_str!("expand_cubes.wesl"), "expand_cubes"),
        JobKind::Model => (include_str!("expand_models.wesl"), "expand_models"),
        JobKind::Liquid => (include_str!("expand_liquids.wesl"), "expand_liquids"),
    };
    let source = kernel(root);
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let sources_layout = layout(&gpu, &source_layout(kind));
    let output_layout = layout(&gpu, &output_layout());
    let pipeline_layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&sources_layout), Some(&output_layout)],
            immediate_size: 0,
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        });
    let source_buffers = [
        (1, storage(&quad_words)),
        (2, storage(&origin_words)),
        (3, storage(&material_words)),
        (7, storage(&[0; 64])),
        (8, storage(&[0; 64])),
        (12, storage(&[0; 64])),
        (13, storage(&stream_words)),
    ];
    let mut source_entries: Vec<_> = source_buffers
        .iter()
        .filter(|(binding, _)| super::pipelines::source_bindings(kind).contains(binding))
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding: *binding,
            resource: buffer.as_entire_binding(),
        })
        .collect();
    source_entries.extend([
        wgpu::BindGroupEntry {
            binding: super::pipelines::ANIMATION_CLOCK_BINDING,
            resource: animation_clock.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: crate::material_shader::BIOME_QUERY_TABLES_BINDING,
            resource: biome_query_tables.as_entire_binding(),
        },
    ]);
    let sources = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &sources_layout,
        entries: &source_entries,
    });
    let jobs = storage(&job_words);
    let remap = storage(&remap_words);
    let output = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &output_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: jobs.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: vertices.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: remap.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params.as_entire_binding(),
            },
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &sources, &[]);
        pass.set_bind_group(1, &output, &[]);
        pass.dispatch_workgroups(
            (plan.jobs[kind as usize].len() as u32).div_ceil(EXPAND_WORKGROUP_SIZE),
            1,
            1,
        );
    }
    gpu.queue.submit([encoder.finish()]);
    let bytes = read_buffer(&gpu, &vertices, vertex_bytes);
    Some(Expanded {
        gpu,
        vertices,
        layout: fixture.layout,
        words: bytemuck::cast_slice(&bytes).to_vec(),
        faces: plan.faces,
        categories: plan.categories.clone(),
    })
}

/// One decoded expanded vertex.
#[derive(Debug)]
struct Decoded {
    position: Vec3,
    colour: [u8; 4],
    uv: [f32; 2],
    mid_uv: [f32; 2],
    lightmap: [f32; 2],
    normal: [i8; 4],
    tangent: [i8; 4],
    block_id: i16,
    render_type: i16,
    mid_block: [i8; 4],
}

fn decode(words: &[u32], vertex: u32) -> Decoded {
    let base = (vertex * IRIS_VERTEX_WORDS) as usize;
    let word = |field: super::vertex::StoredField, offset: u32| {
        words[base + (field.word + offset) as usize]
    };
    let unorm16 = |w: u32| [(w & 0xffff) as f32 / 65535.0, (w >> 16) as f32 / 65535.0];
    let half = |w: u32| [f16_to_f32(w & 0xffff), f16_to_f32(w >> 16)];
    let bytes = |w: u32| w.to_le_bytes();
    let signed = |w: u32| bytes(w).map(|b| b as i8);
    let shorts = |w: u32| [(w & 0xffff) as u16 as i16, (w >> 16) as u16 as i16];
    Decoded {
        position: Vec3::new(
            f32::from_bits(word(POSITION, 0)),
            f32::from_bits(word(POSITION, 1)),
            f32::from_bits(word(POSITION, 2)),
        ),
        colour: bytes(word(COLOR, 0)),
        uv: unorm16(word(UV0, 0)),
        mid_uv: unorm16(word(MID_TEX, 0)),
        lightmap: half(word(LIGHTMAP, 0)),
        normal: signed(word(NORMAL, 0)),
        tangent: signed(word(TANGENT, 0)),
        block_id: shorts(word(BLOCK_ID, 0))[0],
        render_type: shorts(word(BLOCK_ID, 0))[1],
        mid_block: signed(word(MID_BLOCK, 0)),
    }
}

/// IEEE half-precision bits to `f32`.
fn f16_to_f32(bits: u32) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let mantissa = (bits & 0x3ff) as f32;
    match exponent {
        0 => sign * mantissa * 2f32.powi(-24),
        31 => sign * f32::INFINITY,
        _ => sign * (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15),
    }
}

fn atlas_uv(rect: [u32; 4], local: [f32; 2]) -> [f32; 2] {
    [
        (rect[0] as f32 + local[0] * rect[2] as f32) / ATLAS_SIZE[0] as f32,
        (rect[1] as f32 + local[1] * rect[3] as f32) / ATLAS_SIZE[1] as f32,
    ]
}

fn assert_close(actual: &[f32], expected: &[f32], tolerance: f32, what: &str) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, e)| (a - e).abs() <= tolerance),
        "{what}: {actual:?} != {expected:?}"
    );
}

#[test]
fn greedy_top_face_expands_to_per_block_faces_with_java_attributes() {
    let Some(expanded) = expand_fixture("shader-pack cube expansion") else {
        return;
    };
    // Solid: two blocks of the greedy quad; cutout: both sides of the leaf face.
    assert_eq!(expanded.categories, [0..2, 2..4, 4..4]);
    assert_eq!(expanded.faces, 4);
    let offset = Vec3::new(16.0, 0.0, 0.0);
    let top_positions = [
        [
            Vec3::new(2.0, 6.0, 3.0),
            Vec3::new(2.0, 6.0, 4.0),
            Vec3::new(3.0, 6.0, 4.0),
            Vec3::new(3.0, 6.0, 3.0),
        ],
        [
            Vec3::new(3.0, 6.0, 3.0),
            Vec3::new(3.0, 6.0, 4.0),
            Vec3::new(4.0, 6.0, 4.0),
            Vec3::new(4.0, 6.0, 3.0),
        ],
    ];
    let locals = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    // Light at each block corner, bilinear between the greedy quad's four samples:
    // (block, sky) raw Java coordinates and AO brightness.
    let lightmaps = [
        [[48.0, 240.0], [80.0, 240.0], [96.0, 232.0], [32.0, 224.0]],
        [[32.0, 224.0], [96.0, 232.0], [112.0, 224.0], [16.0, 208.0]],
    ];
    let ao = [[1.0, 0.6, 0.8, 0.9], [0.9, 0.8, 1.0, 0.8]];
    let mid_uv = atlas_uv(GRASS_RECT, [0.5, 0.5]);
    for face in 0..2 {
        for corner in 0..4 {
            let vertex = decode(&expanded.words, face * 4 + corner);
            let at = top_positions[face as usize][corner as usize];
            assert_close(
                &vertex.position.to_array(),
                &(at + offset).to_array(),
                1e-6,
                "position",
            );
            assert_close(
                &vertex.uv,
                &atlas_uv(GRASS_RECT, locals[corner as usize]),
                1e-4,
                "uv",
            );
            assert_close(&vertex.mid_uv, &mid_uv, 1e-4, "mc_midTexCoord");
            assert_close(
                &vertex.lightmap,
                &lightmaps[face as usize][corner as usize],
                0.01,
                "lightmap",
            );
            let brightness = (ao[face as usize][corner as usize] * 255.0_f32).round() as u8;
            assert!(
                vertex.colour[..3]
                    .iter()
                    .all(|channel| channel.abs_diff(brightness) <= 1)
                    && vertex.colour[3] == 255,
                "colour carries AO: {:?} vs {brightness}",
                vertex.colour
            );
            assert_eq!(vertex.normal, [0, 127, 0, 0]);
            assert_eq!(
                vertex.tangent,
                [127, 0, 0, -127],
                "+X tangent, mirrored bitangent"
            );
            assert_eq!(i32::from(vertex.block_id), GRASS_BLOCK_ID);
            assert_eq!(
                vertex.render_type, -1,
                "blocks read Iris's BLOCK_RENDER_TYPE in mc_Entity.y"
            );
            let centre = top_positions[face as usize][0] + Vec3::new(0.5, -0.5, 0.5);
            // Iris truncates the offset in 1/64 blocks to signed bytes.
            let expected = ((centre - at) * 64.0).to_array().map(|v| v as i8);
            assert_eq!(
                vertex.mid_block,
                [expected[0], expected[1], expected[2], GRASS_EMISSION as i8],
                "at_midBlock"
            );
        }
    }
}

/// Two-sided cube materials keep a reverse-wound face with the opposite normal.
#[test]
fn two_sided_cube_materials_expand_front_and_back_faces() {
    let Some(expanded) = expand_fixture("shader-pack two-sided expansion") else {
        return;
    };
    assert_eq!(
        expanded.words.len(),
        4 * 4 * IRIS_VERTEX_WORDS as usize,
        "front and back faces"
    );
    let front: Vec<_> = (8..12)
        .map(|vertex| decode(&expanded.words, vertex))
        .collect();
    assert_eq!(front[0].normal, [-127, 0, 0, 0]);
    let back: Vec<_> = (12..16)
        .map(|vertex| decode(&expanded.words, vertex))
        .collect();
    assert_eq!(back[0].normal, [127, 0, 0, 0]);
    assert_eq!(back[0].position, front[0].position);
    assert_eq!(back[3].position, front[1].position);
    assert_eq!(
        front[0].block_id, -1,
        "materials without an id read Iris's -1"
    );
    assert_close(
        &front[0].uv,
        &atlas_uv(LEAF_RECT, [0.0, 1.0]),
        1e-4,
        "west face uv",
    );
}

/// The liquid kernel writes Iris's fluid render type (`mc_Entity.y = 1`) on every corner.
#[test]
fn liquid_faces_carry_the_fluid_render_type() {
    let height = 230_u8;
    let liquid = meshing::PackedLiquidQuad::try_pack(
        [4, 5, 6],
        Face::PositiveY,
        [height; 4],
        GRASS,
        0,
        [0, 0],
        false,
    )
    .unwrap();
    // The record names origin slot 0; its lighting record follows it at record 2.
    let mut streams = liquid.words().to_vec();
    streams[3] = 0;
    streams.extend(words_of(TOP_LIGHT));
    let Some(expanded) = expand_section(
        "shader-pack liquid expansion",
        Fixture {
            kind: JobKind::Liquid,
            cube_quads: &[],
            liquid_quads: &[liquid],
            streams,
            stream_bases: [0, 0, 0, 2],
            layout: TerrainLayout::WIDE,
        },
    ) else {
        return;
    };
    assert_eq!(
        expanded.categories,
        [0..0, 0..0, 0..1],
        "water is translucent"
    );
    let surface = 5.0 + f32::from(height) / 255.0;
    for corner in 0..4 {
        let vertex = decode(&expanded.words, corner);
        assert_eq!(
            vertex.render_type, 1,
            "fluids read Iris's FLUID_RENDER_TYPE in mc_Entity.y"
        );
        assert_eq!(i32::from(vertex.block_id), GRASS_BLOCK_ID);
        assert!(
            (vertex.position.y - surface).abs() < 1e-3,
            "the surface sits at the record's height: {}",
            vertex.position.y
        );
        assert_eq!(vertex.normal, [0, 127, 0, 0]);
    }
}
