//! Execute production model vertices with packed offsets and ordinary quarter turns.
use crate::{
    gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE},
    material_shader, shader_source,
};

const FRAME_BINDING: u32 = material_shader::LAST_CHUNK_BINDING + 1;
const LIGHTMAP_BINDING: u32 = FRAME_BINDING + 7;
const OUTPUT_BINDING: u32 = LIGHTMAP_BINDING + 1;

/// Moves fixture resources beyond the production terrain bindings.
fn source(definitions: &[&str]) -> String {
    let source = shader_source::standalone(include_str!("../../src/model.wgsl"), definitions)
        .replace("@vertex\nfn vertex(", "fn model_vertex(")
        .replace(
            "@builtin(vertex_index) vertex_index: u32",
            "vertex_index: u32",
        )
        .replace(
            "@builtin(instance_index) instance_index: u32",
            "instance_index: u32",
        )
        .replace(
            "@group(1) @binding(0)",
            &format!("@group(0) @binding({LIGHTMAP_BINDING})"),
        );
    let source = (0..7).fold(source, |source, binding| {
        source.replace(
            &format!("@group(2) @binding({binding})"),
            &format!("@group(0) @binding({})", binding + FRAME_BINDING),
        )
    });
    format!(
        "{source}\n{}",
        WITNESS.replace("OUTPUT_BINDING", &OUTPUT_BINDING.to_string())
    )
}

#[test]
fn bamboo_offsets_do_not_rotate_enhanced_surface_normals() {
    let Some(gpu) = fixture_gpu("bamboo Enhanced vertex normals") else {
        return;
    };
    let values = read_vertices(
        &gpu,
        &["ENHANCED"],
        &template_words(),
        &geometry_words(),
        10,
    );
    for case in 0..10 {
        let expected = if case < 4 || case == 8 {
            [-1.0, 0.0, 0.0]
        } else {
            [
                [-1.0, 0.0, 0.0],
                [0.0, 0.0, -1.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
            ][if case == 9 { 1 } else { case - 4 }]
        };
        // Exact to within the driver's normalize rounding, which varies by module context.
        assert!(
            values[case * 3][..3]
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() <= 1.0e-6),
            "case {case}: offset must not become rotation: {:?} != {expected:?}",
            &values[case * 3][..3]
        );
        assert_eq!(values[case * 3][3], 1.0, "fixture vertex must be visible");
        assert_eq!(
            values[case * 3 + 1][3],
            if case < 4 || case == 8 { 1.0 } else { 0.0 },
            "only admitted bamboo selects bounded tile sampling"
        );
        if case >= 8 {
            let expected = if case == 8 {
                [0.5, 0.0, 0.5]
            } else {
                [0.625, 0.25, 0.375]
            };
            assert_eq!(
                &values[case * 3 + 1][..3],
                &expected,
                "explicit offset overrides default geometry"
            );
        }
        if case < 4 {
            assert_eq!(
                values[case * 3 + 2][0],
                case as f32 * meshing::bamboo::STEM_UV_STRIDE
            );
            let x = block_transform::bamboo::OFFSET_MIN
                + case as f32 * block_transform::bamboo::OFFSET_SPAN
                    / (block_transform::bamboo::OFFSET_STEPS - 1) as f32
                + 0.5;
            assert!((values[case * 3 + 1][0] - x).abs() < 1.0e-6);
        }
    }
}

#[test]
fn enhanced_model_projection_retains_wave_displacement_at_distant_origins() {
    let Some(gpu) = fixture_gpu("distant Enhanced model projection") else {
        return;
    };
    read_vertices(
        &gpu,
        &["ENHANCED", "CAMERA_PRECISION_FIXTURE"],
        &template_words(),
        &geometry_words(),
        10,
    );
}

#[test]
fn enhanced_models_receive_the_same_flat_biome_tint_as_vanilla() {
    let Some(gpu) = fixture_gpu("shared Enhanced model vertex tint") else {
        return;
    };
    let vanilla = read_vertices(
        &gpu,
        &["TINT_FIXTURE"],
        &template_words(),
        &geometry_words(),
        10,
    );
    let enhanced = read_vertices(
        &gpu,
        &["ENHANCED", "TINT_FIXTURE"],
        &template_words(),
        &geometry_words(),
        10,
    );
    assert!(
        vanilla[2][0] > 0.1 && vanilla[2][0] < 0.9,
        "fixture must resolve a non-white tint: {:?}",
        vanilla[2]
    );
    for case in 0..10 {
        assert_eq!(
            vanilla[case * 3 + 2],
            enhanced[case * 3 + 2],
            "case {case}: Enhanced must reuse the vanilla vertex tint"
        );
    }
}

/// Allows the output buffer in addition to the renderer's terrain storage buffers.
fn fixture_gpu(name: &str) -> Option<Gpu> {
    Gpu::for_fixture_with_limits(
        name,
        wgpu::Features::empty(),
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: render::required_vertex_storage_buffers() + 1,
            ..Default::default()
        },
    )
}

/// Reads a named integer constant from the composed production shader.
fn shader_constant(module: &naga::Module, name: &str) -> u32 {
    let constant = module
        .constants
        .iter()
        .find(|(_, constant)| constant.name.as_deref() == Some(name))
        .unwrap()
        .1;
    let naga::Expression::Literal(naga::Literal::U32(value)) =
        module.global_expressions[constant.init]
    else {
        panic!("{name} must be an integer literal");
    };
    value
}

/// Executes packed production vertices, including the distant-camera projection witness.
fn read_vertices(
    gpu: &Gpu,
    definitions: &[&str],
    templates: &[u32],
    geometry: &[u32],
    count: usize,
) -> Vec<[f32; 4]> {
    let precision = definitions.contains(&"CAMERA_PRECISION_FIXTURE");
    let tint_fixture = definitions.contains(&"TINT_FIXTURE");
    let mut source = source(definitions);
    if precision {
        source = source.replace("let vertex = model_vertex(0u, id.x);", "let vertex = model_vertex(2u, id.x);")
            .replace("vec4(vertex.normal, f32(vertex.visible & MODEL_VISIBLE))", "vertex.clip_position")
            .replace("vec4(vertex.world_position, f32((vertex.visible & MODEL_BOUNDED_TILE) != 0u))",
                "vec4(foliage_displacement(vec3<f32>(chunk_origins[0].value.xyz) + vec3(0.5, 1.0, 0.6875), CLASS_PLANT, 1.0), 0.0)");
    }
    if tint_fixture {
        let tint = if definitions.contains(&"ENHANCED") {
            "vertex.tint_surface.xyz"
        } else {
            "vertex.tint_gamma"
        };
        source = source.replace("vec4(vertex.uv,0.0,0.0)", &format!("vec4({tint},1.0)"));
    }
    if precision {
        source.push_str(PROJECTION_WITNESS);
    }
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
    let distant = if precision { 1 << 14 } else { 0 };
    let camera = bevy::math::Vec3::splat(distant as f32);
    let view_words = if precision {
        use bevy::math::{Mat4, Vec4};
        crate::gpu_snapshot::view(
            Mat4::from_cols(
                Vec4::ZERO,
                Vec4::new(0.0, -2.0, 0.0, 0.0),
                Vec4::new(8.0, 0.0, 0.0, 0.0),
                Vec4::new(-8.0 * (camera.z + 0.59375), 2.0 * camera.y + 1.0, 0.5, 1.0),
            ),
            camera,
        )
    } else {
        vec![0.0; 104]
    };
    let view = gpu.buffer(&view_words, uniform);
    let origins = gpu.words(&[distant, distant, distant, 0, 0, 0, 0, 0], storage);
    let flags = if tint_fixture {
        assets::MATERIAL_FLAG_FOLIAGE_TINT
    } else {
        0
    };
    let materials = gpu.words(&[0, flags, assets::NO_ANIMATION, 0, 0, 0], storage);
    let records = gpu.words(meshing::PackedBiomeRecord::fallback().words(), storage);
    let mut tint_words = vec![0; 8 + assets::SEASONAL_FOLIAGE_COUNT * 4];
    tint_words[1] = 256 | (512 << 10) | (768 << 20);
    let tints = gpu.words(&tint_words, storage);
    let animations = gpu.words(&[0; 5], storage);
    let frames = gpu.words(&[0], storage);
    let clock = gpu.words(
        &crate::material_shader::world_uniform_words(
            &[0; crate::material_shader::WORLD_CLOCK_WORDS],
        ),
        uniform,
    );
    let templates = gpu.words(templates, storage);
    let geometry = gpu.words(geometry, storage);
    let mut frame_words = [0.0; 256];
    let mut plant_class = 0;
    if precision {
        let parsed = naga::front::wgsl::parse_str(&source).unwrap();
        let frame_type = parsed
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("EnhancedFrame"))
            .unwrap()
            .1;
        let naga::TypeInner::Struct { members, .. } = &frame_type.inner else {
            panic!("Enhanced frame uniform");
        };
        let flags = members
            .iter()
            .find(|member| member.name.as_deref() == Some("flags"))
            .unwrap()
            .offset as usize
            / 4;
        let time = members
            .iter()
            .find(|member| member.name.as_deref() == Some("camera_time"))
            .unwrap()
            .offset as usize
            / 4
            + 3;
        frame_words[flags] = f32::from_bits(shader_constant(&parsed, "FEATURE_WAVING"));
        plant_class = shader_constant(&parsed, "CLASS_PLANT");
        frame_words[time] = 1.0;
    }
    let frame = gpu.buffer(&frame_words, uniform);
    let lightmap = gpu.buffer(
        bytemuck::cast_slice(&render::LightmapInputs::default().build()),
        uniform,
    );
    let class_texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
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
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    if precision {
        gpu.queue.write_texture(
            class_texture.as_image_copy(),
            bytemuck::bytes_of(&plant_class),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: None,
                rows_per_image: None,
            },
            class_texture.size(),
        );
    }
    let classes = class_texture.create_view(&Default::default());
    let bytes = (count * 3 * size_of::<[f32; 4]>()) as u64;
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
    let all_bindings = [
        (0, view.as_entire_binding()),
        (2, origins.as_entire_binding()),
        (3, materials.as_entire_binding()),
        (7, records.as_entire_binding()),
        (8, tints.as_entire_binding()),
        (9, animations.as_entire_binding()),
        (10, frames.as_entire_binding()),
        (
            crate::material_shader::BIOME_QUERY_TABLES_BINDING,
            clock.as_entire_binding(),
        ),
        (12, templates.as_entire_binding()),
        (13, geometry.as_entire_binding()),
        (FRAME_BINDING, frame.as_entire_binding()),
        (
            FRAME_BINDING + 1,
            wgpu::BindingResource::TextureView(&classes),
        ),
        (
            FRAME_BINDING + 3,
            wgpu::BindingResource::TextureView(&classes),
        ),
        (OUTPUT_BINDING, output.as_entire_binding()),
        (LIGHTMAP_BINDING, lightmap.as_entire_binding()),
    ]
    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
    let used = shader_source::bindings_used_by_entry_points(&source, &["witness"], 0);
    let bindings: Vec<_> = all_bindings
        .iter()
        .filter(|entry| used.contains(&entry.binding))
        .cloned()
        .collect();
    assert_eq!(bindings.len(), used.len());
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
        pass.dispatch_workgroups(count as u32, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
    gpu.queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let values: Vec<[f32; 4]> =
        bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();
    if precision {
        let raster = |shader: &str, vertex: &str| {
            let used = shader_source::bindings_used_by_entry_points(
                shader,
                &[vertex, "projection_fragment"],
                0,
            );
            let bindings: Vec<_> = all_bindings
                .iter()
                .filter(|entry| used.contains(&entry.binding))
                .cloned()
                .collect();
            gpu.render(
                shader,
                vertex,
                &[Draw {
                    fragment: "projection_fragment",
                    vertices: 0..6,
                    bindings: &bindings,
                    blend: None,
                    write_depth: false,
                }],
            )
        };
        let expected = raster(&source, "relative_reference_vertex");
        let actual = raster(&source, "projected_vertex");
        let mismatch = actual
            .chunks_exact(4)
            .zip(expected.chunks_exact(4))
            .filter(|(actual, expected)| actual != expected)
            .count();
        assert_eq!(
            mismatch, 0,
            "Enhanced vertex projection must retain distant wave offsets"
        );
        let lossy = source.replace(
            "camera_offset + displacement",
            "camera_offset + (out.world_position - world)",
        );
        assert!(
            raster(&lossy, "projected_vertex") != expected,
            "fixture must detect a world-position round trip"
        );
    }
    if count == 10
        && !precision
        && !tint_fixture
        && let Some(path) = std::env::var_os("CINNABAR_BAMBOO_SHADER_CAPTURE")
    {
        let bindings: Vec<_> = bindings
            .into_iter()
            .filter(|binding| binding.binding != OUTPUT_BINDING)
            .collect();
        let pixels = gpu.render(
            &source,
            "normal_vertex",
            &[Draw {
                fragment: "normal_fragment",
                vertices: 0..60,
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
    values
}

#[test]
fn motion_model_normals_follow_cross_planes_and_sloped_rails() {
    let Some(gpu) = fixture_gpu("motion model geometric normals") else {
        return;
    };
    let mut templates = vec![2, 0, 1, 0, 1, 1, 0];
    for (positions, face) in [
        ([0i16, 0, 0, 256, 0, 256, 256, 256, 256, 0, 256, 0], 8u32),
        (
            [0i16, 0, 0, 0, 256, 256, 256, 256, 256, 256, 0, 0],
            assets::BlockFace::Up.model_quad_face_id(),
        ),
    ] {
        templates.extend(
            positions
                .chunks_exact(2)
                .map(|pair| u32::from(pair[0] as u16) | (u32::from(pair[1] as u16) << 16)),
        );
        templates.extend([0; 4]);
        templates.extend([0, face]);
    }
    let mut geometry: Vec<_> = (0..8)
        .flat_map(|case| meshing::PackedModelDrawRef::new(4 + case, 0).words())
        .collect();
    geometry.resize(48, 0);
    for case in 0..8 {
        let reference = meshing::PackedModelRef::new(
            (case % 4) << 12,
            case / 4,
            (geometry.len() / 2) as u32,
            1,
        )
        .words();
        geometry[16 + case as usize * 4..20 + case as usize * 4].copy_from_slice(&reference);
        geometry.extend([0xffff_ffff; 2]);
    }
    let values = read_vertices(
        &gpu,
        &["ENHANCED_SHADOW", "ENHANCED_MOTION"],
        &templates,
        &geometry,
        8,
    );
    let scale = std::f32::consts::FRAC_1_SQRT_2;
    for (case, expected) in [
        [-scale, 0.0, scale],
        [-scale, 0.0, -scale],
        [scale, 0.0, -scale],
        [scale, 0.0, scale],
        [0.0, scale, -scale],
        [scale, scale, 0.0],
        [0.0, scale, scale],
        [-scale, scale, 0.0],
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(values[case * 3][3], 1.0);
        for (actual, expected) in values[case * 3][..3].iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1e-5,
                "case {case}: actual normal {:?}, expected {expected}",
                &values[case * 3][..3]
            );
        }
    }
}

/// Packs one bamboo face shared by offset and rotation cases.
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

/// Packs bamboo offsets and ordinary model rotations into visible draw references.
fn geometry_words() -> Vec<u32> {
    let mut words: Vec<u32> = (0..10)
        .flat_map(|case| meshing::PackedModelDrawRef::new(5 + case, 0).words())
        .collect();
    words.resize(60, 0);
    for case in 0..10 {
        let variant = if case < 4 {
            case | (4 << 4) | (case << 8)
        } else if case < 8 {
            case - 4
        } else {
            case - 8
        };
        let custom = case >= 8;
        if custom {
            let offset = if case == 8 {
                [0.0; 3]
            } else {
                [0.125, 0.25, -0.125]
            };
            words.extend(offset.map(f32::to_bits));
            words.push(0);
        }
        let reference = meshing::PackedModelRef::new(
            (variant << 12)
                | if custom {
                    meshing::MODEL_REF_FLAG_RANDOM_OFFSET
                } else {
                    0
                },
            u32::from(case >= 4 && case != 8),
            (words.len() / 2) as u32,
            1,
        )
        .words();
        words[20 + case as usize * 4..24 + case as usize * 4].copy_from_slice(&reference);
        words.extend([0xffff_ffff; 2]);
    }
    words
}

const WITNESS: &str = r#"
@group(0) @binding(OUTPUT_BINDING) var<storage, read_write> results: array<vec4<f32>>;
@compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) id: vec3<u32>) {
    let vertex = model_vertex(0u, id.x);
    results[id.x * 3u] = vec4(vertex.normal, f32(vertex.visible & MODEL_VISIBLE));
    results[id.x * 3u + 1u] = vec4(vertex.world_position, f32((vertex.visible & MODEL_BOUNDED_TILE) != 0u));
    results[id.x * 3u + 2u] = vec4(vertex.uv,0.0,0.0);
}
@vertex fn normal_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array(vec2(0.0,0.0),vec2(1.0,0.0),vec2(1.0,1.0),vec2(0.0,1.0));
    let indices = array(0u,1u,2u,0u,2u,3u);
    let case_index = index / 6u;
    var out = model_vertex(0u, case_index);
    let point = (vec2(f32(case_index),0.0) + corners[indices[index % 6u]]) / vec2(10.0,1.0);
    out.clip_position = vec4(point * vec2(2.0,-2.0) + vec2(-1.0,1.0),0.5,1.0);
    return out;
}
@fragment fn normal_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(in.normal * 0.5 + vec3(0.5),1.0);
}
"#;

const PROJECTION_WITNESS: &str = r#"
// Returns the production position through an actual invariant vertex-stage output.
@vertex fn projected_vertex(@builtin(vertex_index) index:u32) -> VertexOutput {
    let corners = array(0u,1u,2u,0u,2u,3u);
    return model_vertex(corners[index],8u);
}
// Projects the same authored corners directly from their local coordinates and wave offsets.
@vertex fn relative_reference_vertex(@builtin(vertex_index) index:u32) -> VertexOutput {
    let indices = array(0u,1u,2u,0u,2u,3u);
    let corners = array(vec3(0.5,0.0,0.5),vec3(0.5,0.0,0.6875),vec3(0.5,1.0,0.6875),vec3(0.5,1.0,0.5));
    let local = corners[indices[index]];
    let displacement = foliage_displacement(vec3<f32>(chunk_origins[0].value.xyz)+local,CLASS_PLANT,local.y);
    let offset = local + displacement;
    var out = model_vertex(indices[index],8u);
    out.clip_position = vec4(8.0*(offset.z-0.59375),1.0-2.0*offset.y,0.5,1.0);
    return out;
}
// Marks covered pixels without changing their geometry.
@fragment fn projection_fragment() -> @location(0) vec4<f32> { return vec4(1.0,0.0,0.0,1.0); }
"#;
