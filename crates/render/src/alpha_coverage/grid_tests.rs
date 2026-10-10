use super::*;

const COLUMNS: u32 = 4;
const CELL_SIDE: u32 = SNAPSHOT_SIDE / COLUMNS;

struct GridCase {
    reference: u32,
    uv: f32,
    gradient: f32,
    coverage: f32,
    special_sampler: bool,
}

/// Places contrasting alpha bands inside and outside the rectangle exposed by a UV grid.
fn grid_atlas(gpu: &Gpu, size: u32) -> wgpu::TextureView {
    let texels: Vec<_> = (0..2)
        .flat_map(|layer| {
            (0..size * size).flat_map(move |pixel| {
                let inside = (size / 4..size / 2).contains(&(pixel % size));
                [255, 0, 0, if inside != (layer == 1) { 255 } else { 0 }]
            })
        })
        .collect();
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("gridded cutout alpha bands"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 2,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &texels,
        )
        .create_view(&Default::default())
}

/// Encodes admitted dimensions and grid metadata through the production reference encoder.
fn grid_cases(model: bool) -> Vec<GridCase> {
    (0..assets::MAX_TEXTURE_PAGES as u32)
        .flat_map(|page| {
            [
                (1, 0.75, 0.0, 1.0, 0),
                (1, 0.375, 0.0, 0.0, 0),
                (1, 1.75, 0.0, if model { 0.0 } else { 1.0 }, 0),
                // Keep the nearest sample inside the opaque texel while its footprint
                // still crosses the contour; exact texel ties vary between GPU backends.
                (1, 0.50001, 1.0 / 32.0, 0.5, 0),
                (2, 1.5, 0.0, if model { 1.0 } else { 0.0 }, 0),
                (2, 1.25, 0.0, if model { 1.0 } else { 0.0 }, 0),
                (0, 0.25, 1.0 / 32.0, 0.5, 0),
                (1, 0.75, 0.0, 0.0, 1),
            ]
            .into_iter()
            .enumerate()
            .map(
                move |(index, (grid, uv, gradient, coverage, layer))| GridCase {
                    reference: crate::material_shader::gpu_grid_texture_ref(
                        assets::TextureRef::new(page, layer).unwrap(),
                        [8; 2],
                        CELL_SIDE << page,
                        grid,
                    ),
                    uv,
                    gradient,
                    coverage,
                    special_sampler: index % 2 != 0,
                },
            )
        })
        .collect()
}

/// Compares the production color sampler and coverage helper on identical controlled inputs.
fn grid_raster(gpu: &Gpu, model: bool, enhanced: bool, samples: u32) -> Vec<u8> {
    let cases = grid_cases(model);
    let words: Vec<_> = cases
        .iter()
        .flat_map(|case| {
            [
                case.reference,
                case.uv.to_bits(),
                case.gradient.to_bits(),
                if case.special_sampler {
                    assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR
                } else {
                    0
                },
            ]
        })
        .collect();
    let data = gpu.words(&words, wgpu::BufferUsages::STORAGE);
    let views = std::array::from_fn::<_, { assets::MAX_TEXTURE_PAGES }, _>(|page| {
        grid_atlas(gpu, CELL_SIDE << page)
    });
    let sampler = gpu.device.create_sampler(&Default::default());
    let mut bindings = Vec::new();
    for (page, view) in views.iter().enumerate() {
        bindings.push(wgpu::BindGroupEntry {
            binding: if enhanced {
                4 + page as u32
            } else {
                crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[page]
            },
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    for binding in std::iter::once(6)
        .chain((model || !enhanced).then_some(crate::material_shader::NATIVE_LEAF_SAMPLER_BINDING))
    {
        bindings.push(wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::Sampler(&sampler),
        });
    }
    bindings.push(wgpu::BindGroupEntry {
        binding: 19,
        resource: data.as_entire_binding(),
    });
    let production = if model {
        include_str!("../model.wgsl")
    } else {
        include_str!("../chunk.wgsl")
    };
    let definitions: &[&str] = if enhanced {
        &[SHADER_DEF, "ENHANCED"]
    } else {
        &[SHADER_DEF]
    };
    let mut source = crate::shader_source::standalone(production, definitions);
    let (sample, footprint, threshold) = if model {
        (
            "var input: VertexOutput; input.uv = uv; input.visible = select(1u, 1u | MODEL_BOUNDED_TILE, c.w != 0u); let sampled = sample_model_ref(input, c.x, dx, dy);",
            "model_alpha_footprint(c.x, uv, dx, dy, sampled)",
            "MODEL_ALPHA_THRESHOLD",
        )
    } else {
        (
            "let sampled = sample_material_texture_ref(c.x, uv, dx, dy, c.w);",
            "cube_alpha_footprint(c.x, uv, dx, dy, sampled, c.w)",
            "TERRAIN_ALPHA_THRESHOLD",
        )
    };
    let output = if samples == 1 {
        format!("if (sampled.a < {threshold}) {{ discard; }} return vec4(sampled.rgb, 1.0);")
    } else {
        format!("let cutout = {footprint}; return vec4(sampled.rgb, cutout.coverage);")
    };
    source.push_str(&format!(
        r#"
@group(0) @binding(19) var<storage, read> grid_cases: array<vec4<u32>>;
@vertex fn grid_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {{
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - vec2(1.0), 0.5, 1.0);
}}
@fragment fn grid_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {{
    let cell = vec2<u32>(position.xy) / {CELL_SIDE}u;
    let c = grid_cases[cell.y * {COLUMNS}u + cell.x];
    let uv = vec2(bitcast<f32>(c.y), 0.125);
    let dx = vec2(bitcast<f32>(c.z), 0.0);
    let dy = vec2(0.0);
    {sample}
    {output}
}}
"#
    ));
    gpu.render_with_state(
        &source,
        "grid_vertex",
        &[Draw {
            fragment: "grid_fragment",
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }],
        RasterState {
            multisample: wgpu::MultisampleState {
                count: samples,
                alpha_to_coverage_enabled: samples > 1,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

/// Checks admitted regions, cube repetition, model scaling, layers and expanded-page footprints.
fn assert_grid_coverage(model: bool) {
    let Some(gpu) = Gpu::for_fixture("gridded cutout color and alpha coverage") else {
        return;
    };
    for enhanced in [false, true] {
        let one = grid_raster(&gpu, model, enhanced, 1);
        let four = grid_raster(&gpu, model, enhanced, 4);
        let pixel = |frame: &[u8], index: usize| {
            let cell = CELL_SIDE as usize;
            let row = index / COLUMNS as usize * cell + cell / 2;
            let column = index % COLUMNS as usize * cell + cell / 2;
            let offset = (row * SNAPSHOT_SIDE as usize + column) * 4;
            <[u8; 3]>::try_from(&frame[offset..offset + 3]).unwrap()
        };
        let clear = pixel(&one, 1);
        assert_ne!(clear, [255, 0, 0]);
        for (index, case) in grid_cases(model).iter().enumerate() {
            let expected = if case.coverage > 0.0 {
                [255, 0, 0]
            } else {
                clear
            };
            assert_eq!(pixel(&one, index), expected, "single-sample case {index}");
            if case.coverage == 0.5 {
                let actual = pixel(&four, index);
                assert!(
                    actual[0] > clear[0] && actual[0] < 255,
                    "Model={model} Enhanced={enhanced} case={index}: scaled footprint retains partial contour coverage: {actual:?}"
                );
            } else {
                assert_eq!(
                    pixel(&four, index),
                    expected,
                    "Model={model} Enhanced={enhanced} case={index}: coverage must address the same alpha texels as color"
                );
            }
        }
    }
}

#[test]
fn gridded_cutout_cube_coverage_matches_color_addressing_and_footprints() {
    assert_grid_coverage(false);
}

#[test]
fn gridded_cutout_model_coverage_matches_color_addressing_and_footprints() {
    assert_grid_coverage(true);
}
