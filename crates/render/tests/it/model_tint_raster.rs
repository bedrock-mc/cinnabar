//! Moving block-constant model tint work must preserve every ordinary terrain pixel.
use crate::{chunk_constants, gpu_snapshot, shader_source};
use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu};
use meshing::{PackedBiomeRecord, PackedModelDrawRef, PackedModelRef};
use std::sync::Arc;
use world::{DecodedBiomeColumn, RawBiomeIds};

const COLUMNS: usize = 6;
const ROWS: usize = 3;
const CASE_COUNT: usize = COLUMNS * ROWS;
const TEXTURE_SIDE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TintRow {
    packed: [u32; 8],
    seasonal: [[f32; 4]; assets::SEASONAL_FOLIAGE_COUNT],
}

/// Covers uniform fast paths, the full lattice and direct seasonal-biome lookup.
fn biome_records() -> [PackedBiomeRecord; ROWS] {
    let storages: [_; 3] = std::array::from_fn(|index| {
        DecodedBiomeColumn::decode(
            0,
            1,
            &[1, (index * 2) as u8],
            &RawBiomeIds { default_biome: 0 },
        )
        .storage(0)
        .expect("one uniform biome storage")
    });
    std::array::from_fn(|index| {
        let neighbours = std::array::from_fn(|slot| {
            Some(Arc::clone(
                &storages[if index == 2 { slot % 3 } else { index }],
            ))
        });
        let record = PackedBiomeRecord::from_neighbourhood(&neighbours, |id| id);
        assert_eq!(record.uniform_tint_index().is_none(), index == 2);
        record
    })
}

/// Supplies different swamp-noise and seasonal cells, including exposed RGB above one.
fn tint_rows() -> Vec<TintRow> {
    (0..3 + assets::TINT_MAP_SIZE as usize)
        .map(|index| {
            let packed_rgb = |offset: usize| {
                let values = [
                    index * 73 + offset,
                    index * 31 + offset * 3,
                    index * 19 + offset * 7,
                ];
                values
                    .into_iter()
                    .enumerate()
                    .fold(0, |word, (channel, value)| {
                        word | (((value % 896 + 64) as u32) << (channel * 10))
                    })
            };
            let flags = if index == 1 {
                assets::BIOME_TINT_FLAG_SWAMP_GRASS | assets::BIOME_TINT_FLAG_SEASONAL_FOLIAGE
            } else {
                0
            };
            TintRow {
                packed: [
                    packed_rgb(11),
                    packed_rgb(23),
                    packed_rgb(47),
                    packed_rgb(61),
                    packed_rgb(79),
                    0,
                    flags,
                    1.0_f32.to_bits(),
                ],
                seasonal: std::array::from_fn(|cell| {
                    [
                        0.16 + cell as f32 * 0.21,
                        0.61 - cell as f32 * 0.07,
                        0.29 + cell as f32 * 0.12,
                        1.0,
                    ]
                }),
            }
        })
        .collect()
}

/// Uses actual packed model references and lighting sidecars rather than synthetic varyings.
fn geometry_words() -> Vec<u32> {
    let ref_word = CASE_COUNT * 2;
    let light_word = ref_word + CASE_COUNT * 4;
    let mut words = (0..CASE_COUNT)
        .flat_map(|index| PackedModelDrawRef::new((ref_word / 4 + index) as u32, 0).words())
        .collect::<Vec<_>>();
    for index in 0..CASE_COUNT {
        let x = [0, 3, 7, 8, 12, 15][index % COLUMNS];
        let transform = x | ((index as u32 % 16) << 4) | ((15 - x) << 8);
        words.extend(
            PackedModelRef::new(transform, index as u32, (light_word / 2 + index) as u32, 1)
                .words(),
        );
    }
    words.extend(crate::solid_terrain_raster::lighting_words(CASE_COUNT));
    words
}

/// Encodes one double-sided, two-block-high quad per material in the production template layout.
fn template_words() -> Vec<u32> {
    let mut words = vec![CASE_COUNT as u32];
    for index in 0..CASE_COUNT {
        words.extend([index as u32, 1, 0]);
    }
    for index in 0..CASE_COUNT {
        let positions: [i16; 12] = [0, 0, 128, 256, 0, 128, 256, 512, 128, 0, 512, 128];
        words.extend(
            positions
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u32::from(pair[0] as u16) | (u32::from(pair[1] as u16) << 16)),
        );
        let uvs: [u16; 8] = [0, 4096, 4096, 4096, 4096, 0, 0, 0];
        words.extend(
            uvs.as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u32::from(pair[0]) | (u32::from(pair[1]) << 16)),
        );
        words.extend([
            index as u32,
            assets::BlockFace::South.model_quad_face_id() | assets::MODEL_QUAD_FLAG_TWO_SIDED,
        ]);
    }
    words
}

/// Covers grass and seasonal foliage with static, animated and second-page textures.
fn material_words() -> Vec<u32> {
    let seasonal = assets::MATERIAL_FLAG_FOLIAGE_TINT | assets::MATERIAL_FLAG_SEASONAL_FOLIAGE;
    let flags = [
        0,
        assets::MATERIAL_FLAG_GRASS_TINT,
        assets::MATERIAL_FLAG_FOLIAGE_TINT,
        seasonal,
        seasonal | assets::MATERIAL_FLAG_EXPOSED_FOLIAGE | assets::MATERIAL_FLAG_BIRCH_FOLIAGE,
        seasonal | assets::MATERIAL_FLAG_EXPOSED_FOLIAGE | assets::MATERIAL_FLAG_EVERGREEN_FOLIAGE,
    ];
    (0..CASE_COUNT)
        .flat_map(|index| {
            [
                if index % 2 == 0 { 0 } else { 1 << 31 },
                flags[index % COLUMNS] | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
                if index % 3 == 0 {
                    0
                } else {
                    assets::NO_ANIMATION
                },
                0,
                0,
                0,
            ]
        })
        .collect()
}

/// Makes holes and the alpha threshold observable in both static and interpolated frames.
fn atlas(gpu: &Gpu) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("model tint raster atlas"),
        size: wgpu::Extent3d {
            width: TEXTURE_SIDE,
            height: TEXTURE_SIDE,
            depth_or_array_layers: 2,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let texels = (0..2 * TEXTURE_SIDE * TEXTURE_SIDE)
        .flat_map(|index| {
            let layer = index / (TEXTURE_SIDE * TEXTURE_SIDE);
            let alpha = if layer == 0 {
                [0, 127, 128, 255]
            } else {
                [255, 255, 0, 255]
            };
            [
                80 + (index % 4) as u8 * 35,
                190 - layer as u8 * 60,
                64 + layer as u8 * 80,
                alpha[index as usize % 4],
            ]
        })
        .collect::<Vec<_>>();
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(TEXTURE_SIDE * 4),
            rows_per_image: Some(TEXTURE_SIDE),
        },
        texture.size(),
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// Calls the production vertex decoder with the renderer's actual indexed quad order.
fn fixture_source(definitions: &[&str], reference: bool) -> String {
    let mut source = shader_source::standalone(include_str!("../../src/model.wesl"), definitions)
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)");
    if reference {
        let start = source
            .find("fn ordinary_world_model_gamma_colour(")
            .expect("ordinary colour helper");
        let end = start
            + source[start..]
                .find("\nfn ordinary_world_model_colour(")
                .expect("linear boundary helper");
        source.replace_range(start..end, REFERENCE_COLOUR);
        let start = source
            .find("@fragment\nfn fragment(")
            .expect("opaque model fragment");
        let end = start
            + source[start..]
                .find("\n@fragment\nfn fragment_blend(")
                .expect("transparent model fragment");
        source.replace_range(start..end, REFERENCE_FRAGMENT);
    }
    source.push_str(
        &RASTER_VERTEX
            .replace(
                "RASTER_INDICES",
                &chunk_constants::STATIC_QUAD_INDICES
                    .map(|index| format!("{index}u"))
                    .join(", "),
            )
            .replace("RASTER_COLUMNS", &format!("{COLUMNS}.0"))
            .replace("RASTER_ROWS", &format!("{ROWS}.0")),
    );
    source
}

/// Walks the executable call graph, including control flow nested inside helper functions.
fn calls_biome_blender(module: &naga::Module, block: &naga::Block) -> bool {
    block.iter().any(|statement| match statement {
        naga::Statement::Call { function, .. } => {
            let function = &module.functions[*function];
            function.name.as_deref() == Some("blended_biome_tint")
                || calls_biome_blender(module, &function.body)
        }
        naga::Statement::Block(block) => calls_biome_blender(module, block),
        naga::Statement::If { accept, reject, .. } => {
            calls_biome_blender(module, accept) || calls_biome_blender(module, reject)
        }
        naga::Statement::Switch { cases, .. } => cases
            .iter()
            .any(|case| calls_biome_blender(module, &case.body)),
        naga::Statement::Loop {
            body, continuing, ..
        } => calls_biome_blender(module, body) || calls_biome_blender(module, continuing),
        _ => false,
    })
}

#[test]
fn ordinary_models_compute_flat_biome_tint_only_in_vertices() {
    for definitions in [&[][..], &["NATIVE_GAMMA_BLEND"][..]] {
        let source = shader_source::standalone(include_str!("../../src/model.wesl"), definitions);
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        for name in ["fragment", "fragment_blend"] {
            let entry = module
                .entry_points
                .iter()
                .find(|entry| entry.stage == naga::ShaderStage::Fragment && entry.name == name)
                .expect("ordinary model fragment entry point");
            assert!(
                !calls_biome_blender(&module, &entry.function.body),
                "{name} must not repeat block-constant biome queries for every covered pixel"
            );
        }
        let vertex = module
            .entry_points
            .iter()
            .find(|entry| entry.stage == naga::ShaderStage::Vertex && entry.name == "vertex")
            .expect("production model vertex entry point");
        assert!(
            calls_biome_blender(&module, &vertex.function.body),
            "model vertices must supply the block's resolved biome tint"
        );
        let result = vertex
            .function
            .result
            .as_ref()
            .expect("model vertex output");
        let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
            panic!("model vertex output must expose its interpolants");
        };
        let tint = members
            .iter()
            .find(|member| member.name.as_deref() == Some("tint_gamma"))
            .expect("ordinary model tint interpolant");
        assert!(matches!(
            tint.binding,
            Some(naga::Binding::Location {
                interpolation: Some(naga::Interpolation::Flat),
                ..
            })
        ));
        assert!(matches!(
            module.types[tint.ty].inner,
            naga::TypeInner::Vector {
                size: naga::VectorSize::Tri,
                scalar: naga::Scalar {
                    kind: naga::ScalarKind::Float,
                    width: 4,
                },
            }
        ));
    }
}

#[test]
fn model_tint_pixels_match_fragment_biome_reference() {
    let Some(gpu) = Gpu::for_fixture("model tint raster") else {
        return;
    };
    let storage = wgpu::BufferUsages::STORAGE;
    let uniform = wgpu::BufferUsages::UNIFORM;
    let records = biome_records();
    let mut record_words = Vec::new();
    let offsets: [_; ROWS] = std::array::from_fn(|index| {
        let start = record_words.len() as u32;
        record_words.extend(records[index].words());
        start
    });
    let origins = (0..CASE_COUNT)
        .flat_map(|index| {
            let position: [i32; 3] = if index % 2 == 0 {
                [-32, 64, -48]
            } else {
                [16, 64, 16]
            };
            [
                position[0] as u32,
                position[1] as u32,
                position[2] as u32,
                offsets[index / COLUMNS],
                0,
                0,
                0,
                0,
            ]
        })
        .collect::<Vec<_>>();
    let view = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::new(0.0, 68.0, 0.0)),
        uniform,
    );
    let origins = gpu.words(&origins, storage);
    let materials = gpu.words(&material_words(), storage);
    let records = gpu.words(&record_words, storage);
    let tints = gpu.words(bytemuck::cast_slice(&tint_rows()), storage);
    let query_tables = gpu.words(&meshing::biome_lattice::query_table_words(), uniform);
    let animations = gpu.words(
        &[0, 2, 4, assets::ANIMATION_FLAG_BLEND, 1.0_f32.to_bits()],
        storage,
    );
    let frames = gpu.words(&[0, 1], storage);
    let clock = gpu.words(
        &[1, 0.25_f32.to_bits(), 0, 0],
        uniform | wgpu::BufferUsages::COPY_DST,
    );
    let templates = gpu.words(&template_words(), storage);
    let geometry = gpu.words(&geometry_words(), storage);
    let mut atmosphere = [0.0; 36];
    atmosphere[16..19].copy_from_slice(&[0.24, 0.31, 0.47]);
    atmosphere[19] = 4.0;
    atmosphere[20] = 180.0;
    let atmosphere = gpu.buffer(&atmosphere, uniform);
    let lightmap = gpu.buffer(
        bytemuck::cast_slice(&render::LightmapInputs::default().build()),
        uniform,
    );
    let atlas = atlas(&gpu);
    let sampler = gpu.device.create_sampler(&Default::default());
    let bindings = [
        (0, view.as_entire_binding()),
        (2, origins.as_entire_binding()),
        (3, materials.as_entire_binding()),
        (
            crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
            wgpu::BindingResource::TextureView(&atlas),
        ),
        (
            crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
            wgpu::BindingResource::TextureView(&atlas),
        ),
        (
            crate::material_shader::NATIVE_LEAF_SAMPLER_BINDING,
            wgpu::BindingResource::Sampler(&sampler),
        ),
        (6, wgpu::BindingResource::Sampler(&sampler)),
        (7, records.as_entire_binding()),
        (8, tints.as_entire_binding()),
        (9, animations.as_entire_binding()),
        (10, frames.as_entire_binding()),
        (11, clock.as_entire_binding()),
        (12, templates.as_entire_binding()),
        (13, geometry.as_entire_binding()),
        (15, atmosphere.as_entire_binding()),
        (
            crate::material_shader::BIOME_QUERY_TABLES_BINDING,
            query_tables.as_entire_binding(),
        ),
        (20, lightmap.as_entire_binding()),
    ]
    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
    for definitions in [&[][..], &["NATIVE_GAMMA_BLEND"][..]] {
        let reference = fixture_source(definitions, true);
        let candidate = fixture_source(definitions, false);
        for clock_words in [[1, 0.25_f32.to_bits(), 0, 0], [2, 0, 0, 0]] {
            gpu.queue
                .write_buffer(&clock, 0, bytemuck::cast_slice(&clock_words));
            for fragment in ["fragment", "fragment_blend"] {
                let draws = [Draw {
                    fragment,
                    vertices: 0..CASE_COUNT as u32 * 6,
                    bindings: &bindings,
                    blend: (fragment == "fragment_blend")
                        .then_some(wgpu::BlendState::ALPHA_BLENDING),
                    write_depth: fragment == "fragment",
                }];
                let raster = if fragment == "fragment_blend"
                    && definitions.contains(&"NATIVE_GAMMA_BLEND")
                {
                    Gpu::render
                } else {
                    Gpu::render_srgb
                };
                let expected = raster(&gpu, &reference, "raster_model_vertex", &draws);
                let actual = raster(&gpu, &candidate, "raster_model_vertex", &draws);
                // The flat vertex tint and the per-fragment reference may round one unit apart.
                let mismatch = expected
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(actual.as_chunks::<4>().0.iter())
                    .filter(|(left, right)| {
                        left.iter().zip(*right).any(|(l, r)| l.abs_diff(*r) > 1)
                    })
                    .count();
                assert_eq!(
                    mismatch, 0,
                    "{fragment}, definitions {definitions:?}, clock {clock_words:?}"
                );
                if fragment == "fragment" {
                    let side = gpu_snapshot::SNAPSHOT_SIDE as usize;
                    let offset = ((side / (ROWS * 2)) * side + side / (COLUMNS * 8)) * 4;
                    // The second cell's first texel stays fully transparent without animation.
                    let background = ((side / (ROWS * 2)) * side + 9 * side / (COLUMNS * 8)) * 4;
                    if clock_words[0] == 2 {
                        assert_eq!(actual[offset + 3], 128, "alpha exactly 0.5 survives");
                        assert_ne!(
                            &actual[offset..offset + 4],
                            &actual[background..background + 4],
                            "the threshold texel must replace the clear colour"
                        );
                    } else {
                        assert_eq!(
                            &actual[offset..offset + 4],
                            &actual[background..background + 4],
                            "alpha below 0.5 leaves the clear colour untouched"
                        );
                    }
                }
                for case in 0..CASE_COUNT {
                    let row = case / COLUMNS;
                    let column = case % COLUMNS;
                    let side = gpu_snapshot::SNAPSHOT_SIDE as usize;
                    let x = (column * 8 + 7) * side / (COLUMNS * 8);
                    let y = (row * 2 + 1) * side / (ROWS * 2);
                    let offset = (y * side + x) * 4;
                    assert_ne!(
                        &actual[offset..offset + 4],
                        &actual[..4],
                        "case {case} must shade visible pixels"
                    );
                }
            }
        }
    }
}

const RASTER_VERTEX: &str = r#"
// Pulls the production model vertex before arranging the fixture cells.
@vertex fn raster_model_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let indices = array<u32, 6>(RASTER_INDICES);
    let corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
    let case_index = index / 6u;
    let corner = indices[index % 6u];
    var out = model_vertex(case_index * 4u + corner, case_index);
    let cell = vec2(f32(case_index % u32(RASTER_COLUMNS)), f32(case_index / u32(RASTER_COLUMNS)));
    let position = (cell + corners[corner]) / vec2(RASTER_COLUMNS, RASTER_ROWS);
    out.clip_position = vec4(position * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.5, 1.0);
    return out;
}
"#;

const REFERENCE_COLOUR: &str = r#"
// Resolves each fragment's tint at its owning block before lighting and fog.
fn ordinary_world_model_gamma_colour(in: VertexOutput, sampled_gamma: vec4<f32>) -> vec4<f32> {
    var tint_gamma = vec3(1.0);
    let tint_kind = in.material_flags & 0x30u;
    if (tint_kind != 0u) {
        tint_gamma = blended_biome_tint_gamma(tint_kind, in.material_flags, in.biome_record, in.local_position, in.world_origin).rgb;
    }
    let lit_gamma = ((sampled_gamma.rgb * tint_gamma) * in.native_ao_face) * terrain_light_colour(in.native_light_levels);
    let fog_gamma = tint_to_gamma(vec4(atmosphere.fog_color_start.rgb, 1.0)).rgb;
    return vec4(mix(lit_gamma, fog_gamma, distance_fog_amount(in.world_position)), sampled_gamma.a);
}
"#;

const REFERENCE_FRAGMENT: &str = r#"
// Samples and mixes full frame colour before rejecting uncovered fragments.
@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) front_facing: bool,
) -> @location(0) vec4<f32> {
    if (in.visibility.x == 0u) { discard; }
    if (!front_facing && in.visibility.y == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    var sampled = sample_model_ref(in, in.current_texture, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_model_ref(in, in.next_texture, dx, dy), in.frame_blend);
    }
    if (sampled.a < 0.5) { discard; }
    return ordinary_world_model_colour(in, sampled);
}
"#;
