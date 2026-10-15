use super::*;

const ALPHAS: [u8; 6] = [0, 64, 127, 129, 192, 255];
const COLUMNS: usize = ALPHAS.len() + 1;

/// Uploads threshold controls and a binary leaf mask whose distant mips are three-quarters covered.
fn leaf_atlas(gpu: &Gpu) -> wgpu::TextureView {
    let mut bytes = Vec::new();
    for layer in 0..COLUMNS {
        let base: Vec<_> = (0..assets::TILE_SIZE * assets::TILE_SIZE)
            .flat_map(|pixel| {
                let alpha = ALPHAS.get(layer).copied().unwrap_or_else(|| {
                    if pixel.is_multiple_of(2) && (pixel / assets::TILE_SIZE).is_multiple_of(2) {
                        0
                    } else {
                        255
                    }
                });
                [255, 0, 0, alpha]
            })
            .collect();
        for mip in assets::build_legacy_terrain_mip_chain(&base, assets::TILE_SIZE).unwrap() {
            bytes.extend_from_slice(&mip.rgba8);
        }
    }
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("leaf cutoff and distant canopy mips"),
                size: wgpu::Extent3d {
                    width: assets::TILE_SIZE,
                    height: assets::TILE_SIZE,
                    depth_or_array_layers: COLUMNS as u32,
                },
                mip_level_count: assets::MIP_COUNT,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &bytes,
        )
        .create_view(&Default::default())
}

/// Samples each physical page through the world-leaf sampler and production cutout helper.
fn leaf_mip_raster(gpu: &Gpu, samples: u32) -> Vec<u8> {
    let atlas = leaf_atlas(gpu);
    let sampler = gpu
        .device
        .create_sampler(&crate::material_shader::native_leaf_sampler_descriptor());
    let mut bindings: Vec<_> = crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS
        .into_iter()
        .map(|binding| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(&atlas),
        })
        .collect();
    bindings.push(wgpu::BindGroupEntry {
        binding: crate::material_shader::NATIVE_LEAF_SAMPLER_BINDING,
        resource: wgpu::BindingResource::Sampler(&sampler),
    });
    bindings.push(wgpu::BindGroupEntry {
        binding: 6,
        resource: wgpu::BindingResource::Sampler(&sampler),
    });
    let references: Vec<_> = (0..assets::MAX_TEXTURE_PAGES as u32)
        .flat_map(|page| {
            (0..COLUMNS as u32).map(move |layer| {
                format!("{}u", assets::TextureRef::new(page, layer).unwrap().raw())
            })
        })
        .collect();
    let mut source = crate::shader_source::standalone(include_str!("../chunk.wesl"), &[SHADER_DEF]);
    source.push_str(&format!(
        r#"
@vertex fn leaf_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {{
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - vec2(1.0), 0.5, 1.0);
}}
@fragment fn leaf_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {{
    let column = min(u32(position.x * {COLUMNS}.0 / {SNAPSHOT_SIDE}.0), {last_column}u);
    let row = min(u32(position.y * {rows}.0 / {SNAPSHOT_SIDE}.0), {last_row}u);
    let references = array({references});
    let reference = references[(row / 2u) * {COLUMNS}u + column];
    // Alternate magnification and minification, with texel ties avoided in the base level.
    let uv = vec2(0.28125, 0.28125);
    let gradient = select(0.0, 1.0, (row & 1u) != 0u);
    let dx = vec2(gradient, 0.0);
    let dy = vec2(0.0, gradient);
    let flags = {flags}u;
    let sampled = sample_material_texture_ref(reference, uv, dx, dy, flags);
    let cutout = cube_alpha_footprint(reference, uv, dx, dy, sampled, flags);
    if ({single} && sampled.a < TERRAIN_ALPHA_THRESHOLD) {{ discard; }}
    let alpha = select(cutout.coverage, 1.0, {single});
    return vec4(sampled.rgb, alpha);
}}
"#,
        last_column = COLUMNS - 1,
        rows = assets::MAX_TEXTURE_PAGES * 2,
        last_row = assets::MAX_TEXTURE_PAGES * 2 - 1,
        references = references.join(", "),
        flags = assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR,
        single = samples == 1,
    ));
    gpu.render_with_state(
        &source,
        "leaf_vertex",
        &[Draw {
            fragment: "leaf_fragment",
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

#[test]
fn world_leaf_cutoff_keeps_distant_canopies_opaque_with_msaa() {
    let Some(gpu) = Gpu::for_fixture("world leaf mip coverage") else {
        return;
    };
    let one = leaf_mip_raster(&gpu, 1);
    let four = leaf_mip_raster(&gpu, 4);
    crate::gpu_snapshot::save("leaf-mips-1x", &one);
    crate::gpu_snapshot::save("leaf-mips-4x", &four);
    assert!(
        one == four,
        "leaf alpha selects opaque or uncovered samples at both distances; dense mips must not leak the background under MSAA"
    );
    let side = SNAPSHOT_SIDE as usize;
    for row in 0..assets::MAX_TEXTURE_PAGES * 2 {
        for column in 0..COLUMNS {
            let y = (row * 2 + 1) * side / (assets::MAX_TEXTURE_PAGES * 4);
            let x = (column * 2 + 1) * side / (COLUMNS * 2);
            let offset = (y * side + x) * 4;
            let opaque = (3..ALPHAS.len()).contains(&column);
            // The binary mask's chosen base texel is transparent; its distant mip passes the cutoff.
            let opaque = opaque || (column == ALPHAS.len() && row % 2 == 1);
            assert_eq!(
                one[offset..offset + 3] == [255, 0, 0],
                opaque,
                "page {}, minified {}, alpha control {column}",
                row / 2,
                row % 2 == 1,
            );
        }
    }
}
