//! Uniform cube tints may skip fragment lookup; mixed and swamp grass retain per-block colour.
use crate::{chunk_constants, gpu_snapshot, material_shader, shader_source};
use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu};
use meshing::{Face, PackedBiomeRecord, PackedQuad};
use std::sync::Arc;
use world::{DecodedBiomeColumn, RawBiomeIds};

const COLUMNS: usize = 6;
const ROWS: usize = 6;
const CASES: usize = COLUMNS * ROWS;

/// Encodes normal, swamp/seasonal and mixed descriptors through the production biome packer.
fn records() -> [PackedBiomeRecord; 3] {
    let storages: [_; 3] = std::array::from_fn(|index| {
        DecodedBiomeColumn::decode(
            0,
            1,
            &[1, (index * 2) as u8],
            &RawBiomeIds { default_biome: 0 },
        )
        .storage(0)
        .expect("fixture biome storage")
    });
    std::array::from_fn(|row| {
        let neighbours = std::array::from_fn(|index| {
            Some(Arc::clone(
                &storages[if row == 2 { index % 3 } else { row }],
            ))
        });
        let record = PackedBiomeRecord::from_neighbourhood(&neighbours, |id| id);
        assert_eq!(record.uniform_tint_index().is_none(), row == 2);
        record
    })
}

/// Makes seasonal exposure, foliage species and spatial swamp noise visibly different.
fn tint_words() -> Vec<u32> {
    let mut words = Vec::new();
    for index in 0..3 + assets::TINT_MAP_SIZE {
        let packed = |phase: u32| {
            let components = [
                index * 173 + phase,
                index * 47 + phase * 3,
                index * 23 + phase * 7,
            ];
            components
                .into_iter()
                .enumerate()
                .fold(0, |word, (channel, value)| {
                    word | ((value % 900 + 100) << (channel * 10))
                })
        };
        let flags = if index == 1 {
            assets::BIOME_TINT_FLAG_SWAMP_GRASS | assets::BIOME_TINT_FLAG_SEASONAL_FOLIAGE
        } else {
            0
        };
        words.extend([
            packed(31),
            packed(79),
            packed(131),
            packed(191),
            packed(257),
            0,
            flags,
            1.0_f32.to_bits(),
        ]);
        for cell in 0..assets::SEASONAL_FOLIAGE_COUNT {
            words.extend(
                [
                    0.12 + cell as f32 * 0.23,
                    0.7 - cell as f32 * 0.1,
                    0.24 + cell as f32 * 0.11,
                    1.0,
                ]
                .map(f32::to_bits),
            );
        }
    }
    words
}

/// Covers overlays, native leaves, regular foliage and exposed seasonal species.
fn material_flags() -> [u32; COLUMNS] {
    let leaf = assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR | assets::MATERIAL_FLAG_FOLIAGE_TINT;
    let seasonal = leaf | assets::MATERIAL_FLAG_SEASONAL_FOLIAGE;
    [
        assets::MATERIAL_FLAG_GRASS_TINT | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        assets::MATERIAL_FLAG_GRASS_TINT | assets::MATERIAL_FLAG_OVERLAY_MASK,
        leaf | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        seasonal | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        seasonal
            | assets::MATERIAL_FLAG_EXPOSED_FOLIAGE
            | assets::MATERIAL_FLAG_BIRCH_FOLIAGE
            | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        seasonal
            | assets::MATERIAL_FLAG_EXPOSED_FOLIAGE
            | assets::MATERIAL_FLAG_EVERGREEN_FOLIAGE
            | assets::MATERIAL_FLAG_ALPHA_CUTOUT,
    ]
    .map(|flags| flags | assets::MATERIAL_FLAG_TWO_SIDED)
}

/// Ordinary terrain samples the encoded-byte view of the sRGB atlas.
fn atlas(gpu: &Gpu) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cube tint raster atlas"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 2,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    let texels = (0..32)
        .flat_map(|index| {
            [
                80 + (index % 4) * 40,
                210 - (index / 16) * 70,
                64 + (index % 4) * 20,
                [0, 127, 128, 255][index as usize % 4],
            ]
        })
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
    texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// Replaces only the colour helper with its complete fragment-tint reference.
fn source(reference: bool) -> String {
    let mut source = shader_source::standalone(include_str!("../../src/chunk.wgsl"), &[])
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)");
    if reference {
        let start = source.find("fn shade_cube(").expect("cube shade helper");
        source.truncate(start);
        source.push_str(REFERENCE_SHADE);
    }
    source.push_str(
        &VERTEX
            .replace(
                "INDICES",
                &chunk_constants::STATIC_QUAD_INDICES
                    .map(|index| format!("{index}u"))
                    .join(", "),
            )
            .replace("COLUMNS", &format!("{COLUMNS}.0"))
            .replace("ROWS", &format!("{ROWS}.0")),
    );
    source
}

/// Follows calls through nested control flow without depending on source spelling or layout.
fn calls(module: &naga::Module, statement: &naga::Statement, name: &str) -> bool {
    let block_calls =
        |block: &naga::Block| block.iter().any(|statement| calls(module, statement, name));
    match statement {
        naga::Statement::Call { function, .. } => {
            let function = &module.functions[*function];
            function.name.as_deref() == Some(name) || block_calls(&function.body)
        }
        naga::Statement::Block(block) => block_calls(block),
        naga::Statement::If { accept, reject, .. } => block_calls(accept) || block_calls(reject),
        naga::Statement::Switch { cases, .. } => cases.iter().any(|case| block_calls(&case.body)),
        naga::Statement::Loop {
            body, continuing, ..
        } => block_calls(body) || block_calls(continuing),
        _ => false,
    }
}

#[test]
fn uniform_cube_tints_return_before_fragment_biome_work() {
    let source = shader_source::standalone(include_str!("../../src/chunk.wgsl"), &[]);
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let helper = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("ordinary_cube_tint_gamma"))
        .expect("ordinary cubes must reuse their admitted vertex tint")
        .1;
    let (gate, accept) = helper
        .body
        .iter()
        .enumerate()
        .find_map(|(index, statement)| {
            if let naga::Statement::If { accept, .. } = statement {
                Some((index, accept))
            } else {
                None
            }
        })
        .expect("cached tint must return before the fallback lookup");
    assert!(
        !helper.body.iter().take(gate).any(|statement| calls(
            &module,
            statement,
            "blended_biome_tint"
        )),
        "the cached branch must not follow an unconditional biome query"
    );
    assert!(
        accept
            .iter()
            .any(|statement| matches!(statement, naga::Statement::Return { .. }))
    );
    assert!(
        !accept
            .iter()
            .any(|statement| calls(&module, statement, "blended_biome_tint"))
    );
    assert!(
        helper
            .body
            .iter()
            .any(|statement| calls(&module, statement, "blended_biome_tint")),
        "mixed records retain the shared blender"
    );
    let vertex = module
        .entry_points
        .iter()
        .find(|entry| entry.stage == naga::ShaderStage::Vertex)
        .unwrap();
    assert!(vertex.function.body.iter().any(|statement| calls(
        &module,
        statement,
        "uniform_biome_tint_gamma"
    )));
    let result = vertex.function.result.as_ref().unwrap();
    let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
        panic!("cube vertex outputs");
    };
    assert!(members.iter().any(|member| matches!(
        member.binding,
        Some(naga::Binding::Location {
            location: 12,
            interpolation: Some(naga::Interpolation::Flat),
            ..
        })
    )));
}

#[test]
fn enhanced_and_shadow_cube_vertices_do_not_resolve_biome_tint() {
    for definitions in [
        &["ENHANCED"][..],
        &["ENHANCED_SHADOW"][..],
        &["OPAQUE_OVERDRAW"][..],
    ] {
        let source = shader_source::standalone(include_str!("../../src/chunk.wgsl"), definitions);
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        let vertex = module
            .entry_points
            .iter()
            .find(|entry| entry.stage == naga::ShaderStage::Vertex)
            .unwrap();
        for name in ["uniform_biome_tint_gamma", "blended_biome_tint"] {
            assert!(
                !vertex
                    .function
                    .body
                    .iter()
                    .any(|statement| calls(&module, statement, name)),
                "{definitions:?} vertex reaches {name}"
            );
        }
    }
}

#[test]
fn uniform_cube_pixels_match_fragment_reference_and_mixed_fallback() {
    render_fixture(false);
}

#[test]
fn uniform_cube_cache_admits_only_position_independent_descriptors() {
    render_fixture(true);
}

/// Checks either exact old pixels or observable admission without requiring the new varying in both.
fn render_fixture(cache_admission: bool) {
    let Some(gpu) = Gpu::for_fixture("uniform cube tint raster") else {
        return;
    };
    let storage = wgpu::BufferUsages::STORAGE;
    let uniform = wgpu::BufferUsages::UNIFORM;
    let mut biome_words = Vec::new();
    let descriptors = records();
    let mut offsets = descriptors
        .iter()
        .map(|record| {
            let offset = biome_words.len() as u32;
            biome_words.extend(record.words());
            offset
        })
        .collect::<Vec<_>>();
    for invalid_magic in [true, false] {
        let mut words = descriptors[0].words().to_vec();
        if invalid_magic {
            words[0] = 0;
        } else {
            words[1] = u32::MAX - 1;
        }
        offsets.push(biome_words.len() as u32);
        biome_words.extend(words);
    }
    offsets.push(u32::MAX);
    assert_eq!(offsets.len(), ROWS);
    let quads = (0..CASES)
        .map(|index| {
            PackedQuad::new(
                [0, (index % 8) as u8, 0],
                Face::ALL[index % COLUMNS],
                16,
                8,
                (index % COLUMNS) as u32,
            )
        })
        .collect::<Vec<_>>();
    let origins = (0..CASES)
        .flat_map(|index| {
            [
                (-320_i32) as u32,
                64,
                (-480_i32) as u32,
                offsets[index / COLUMNS],
                0,
                0,
                0,
                0,
            ]
        })
        .collect::<Vec<_>>();
    let materials = material_flags()
        .into_iter()
        .enumerate()
        .flat_map(|(index, flags)| {
            [
                0,
                flags,
                if index % 2 == 0 {
                    assets::NO_ANIMATION
                } else {
                    0
                },
                0,
                0,
                0,
            ]
        })
        .collect::<Vec<_>>();
    let view = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::new(-312.0, 68.0, -472.0)),
        uniform,
    );
    let quads = gpu.words(
        &quads.iter().flat_map(PackedQuad::words).collect::<Vec<_>>(),
        storage,
    );
    let origins = gpu.words(&origins, storage);
    let materials = gpu.words(&materials, storage);
    let records = gpu.words(&biome_words, storage);
    let tints = gpu.words(&tint_words(), storage);
    let animations = gpu.words(
        &[0, 2, 4, assets::ANIMATION_FLAG_BLEND, 1.0_f32.to_bits()],
        storage,
    );
    let frames = gpu.words(&[0, 1], storage);
    let clock = gpu.words(&[1, 0.25_f32.to_bits(), 0, 0], uniform);
    let streams = gpu.words(&crate::solid_terrain_raster::lighting_words(CASES), storage);
    let mut atmosphere = [0.0; 32];
    atmosphere[16..19].copy_from_slice(&[0.13, 0.22, 0.31]);
    atmosphere[20] = 64.0;
    let atmosphere = gpu.buffer(&atmosphere, uniform);
    let lightmap = gpu.buffer(
        bytemuck::cast_slice(&render::LightmapInputs::default().build()),
        uniform,
    );
    let native_atlas = atlas(&gpu);
    let sampler = gpu.device.create_sampler(&Default::default());
    let native_sampler = gpu
        .device
        .create_sampler(&material_shader::native_leaf_sampler_descriptor());
    let bindings = [
        (0, view.as_entire_binding()),
        (1, quads.as_entire_binding()),
        (2, origins.as_entire_binding()),
        (3, materials.as_entire_binding()),
        (6, wgpu::BindingResource::Sampler(&sampler)),
        (7, records.as_entire_binding()),
        (8, tints.as_entire_binding()),
        (9, animations.as_entire_binding()),
        (10, frames.as_entire_binding()),
        (11, clock.as_entire_binding()),
        (13, streams.as_entire_binding()),
        (15, atmosphere.as_entire_binding()),
        (20, lightmap.as_entire_binding()),
        (
            material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
            wgpu::BindingResource::TextureView(&native_atlas),
        ),
        (
            material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
            wgpu::BindingResource::TextureView(&native_atlas),
        ),
        (
            material_shader::NATIVE_LEAF_SAMPLER_BINDING,
            wgpu::BindingResource::Sampler(&native_sampler),
        ),
    ]
    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
    let draw = |fragment| Draw {
        fragment,
        vertices: 0..CASES as u32 * 6,
        bindings: &bindings,
        blend: None,
        write_depth: true,
    };
    let reference = source(true);
    let candidate = source(false);
    if !cache_admission {
        for fragment in ["fragment", "fragment_solid"] {
            let expected = gpu.render_srgb(&reference, "tint_cube_vertex", &[draw(fragment)]);
            let actual = gpu.render_srgb(&candidate, "tint_cube_vertex", &[draw(fragment)]);
            let mismatched = expected
                .chunks_exact(4)
                .zip(actual.chunks_exact(4))
                .filter(|(left, right)| left != right)
                .count();
            assert_eq!(
                mismatched, 0,
                "{fragment}: cached and per-fragment tint must be identical"
            );
            assert!(
                actual.chunks_exact(4).any(|pixel| pixel != &actual[..4]),
                "fixture must draw coloured terrain"
            );
        }
        return;
    }
    let markers = gpu.render_srgb(
        &format!("{candidate}\n{MARKER}"),
        "tint_cube_vertex",
        &[draw("cache_marker")],
    );
    let side = gpu_snapshot::SNAPSHOT_SIDE as usize;
    for case in 0..CASES {
        let row = case / COLUMNS;
        let column = case % COLUMNS;
        let offset = (((row * 2 + 1) * side / (ROWS * 2)) * side
            + (column * 2 + 1) * side / (COLUMNS * 2))
            * 4;
        let cached = row == 0 || (row == 1 && column >= 2) || row == 4;
        assert_eq!(
            markers[offset + 3],
            if cached { 255 } else { 0 },
            "case {case}: uniform admission and fallback"
        );
    }
}

const VERTEX: &str = r#"
// Keeps real greedy positions and UVs while laying each face in an independently visible cell.
@vertex fn tint_cube_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let indices = array<u32, 6>(INDICES);
    let corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
    let quad = index / 6u;
    let corner = indices[index % 6u];
    var out = cube_vertex(quad * 4u + corner, quad);
    let cell = vec2(f32(quad % u32(COLUMNS)), f32(quad / u32(COLUMNS)));
    let position = (cell + corners[corner]) / vec2(COLUMNS, ROWS);
    out.clip_position = vec4(position * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.5, 1.0);
    return out;
}
"#;

const REFERENCE_SHADE: &str = r#"
// Resolves each covered pixel's owning block before the existing native colour transfer.
fn shade_cube(in: VertexOutput, sampled: vec4<f32>) -> vec4<f32> {
    var tint_gamma = vec3(1.0);
    let tint_kind = in.material_flags & 0x30u;
    if (tint_kind != 0u) {
        tint_gamma = blended_biome_tint_gamma(
            tint_kind,
            in.material_flags,
            in.biome_record,
            in.local_position - in.normal * 0.001,
            in.world_position - in.local_position,
        ).rgb;
    }
    let native_colour = native_cube_colour(
        sampled,
        in.material_flags,
        tint_gamma,
        in.native_ao_face,
        terrain_light_colour(in.native_light_levels),
        atmosphere.fog_color_start.rgb,
        distance_fog_amount(in.world_position),
    );
    return native_colour;
}
"#;

const MARKER: &str = r#"
// Alpha reports cache admission while RGB retains all production texture and lighting bindings.
@fragment fn cache_marker(in: VertexOutput) -> @location(0) vec4<f32> {
    let colour = shade_cube(in, sample_cube_texture(in, dpdx(in.uv), dpdy(in.uv)));
    return vec4(colour.rgb, in.uniform_tint_gamma.a);
}
"#;
