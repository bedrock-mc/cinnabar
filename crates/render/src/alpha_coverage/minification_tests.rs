use super::*;

/// Draws a minified sprite before an opaque sprite behind it to expose stray colour and depth.
fn minified_item_raster(gpu: &Gpu, samples: u32, tint_alpha: f32) -> Vec<u8> {
    let mut source = crate::shader_source::standalone(
        include_str!("../dropped_item.wgsl"),
        if samples > 1 { &[SHADER_DEF] } else { &[] },
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
    .replace("@group(1) @binding(1)", "@group(0) @binding(21)");
    source.push_str(&format!(
        r#"
@vertex fn minified_item_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {{
    let uv = vec2(f32(((index % 3u) << 1u) & 2u), f32((index % 3u) & 2u));
    let background = index >= 3u;
    var out: VertexOutput;
    out.position = vec4(uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), select(0.75, 0.25, background), 1.0);
    out.uv = uv;
    out.layer = select(0u, 1u, background);
    out.shade = 1.0;
    out.levels = vec2(15u);
    out.color = vec4(1.0, 1.0, 1.0, select({tint_alpha}, 1.0, background));
    out.overlay = vec4(0.0);
    out.world_position = vec3(0.0);
    return out;
}}
"#
    ));
    let width = SNAPSHOT_SIDE * 4;
    let mut texels = Vec::with_capacity(width as usize * 8);
    for column in 0..width {
        let alpha = [0, 16, 64, 255][(column / SNAPSHOT_SIDE) as usize];
        texels.extend([255, 0, 0, alpha]);
    }
    for _ in 0..width {
        texels.extend([0, 255, 0, 255]);
    }
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("minified dropped-item alpha fixture"),
            size: wgpu::Extent3d {
                width,
                height: 1,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &texels,
    );
    let sprites = texture.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&Default::default());
    let view = gpu.buffer(
        &crate::gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let light = gpu.buffer(&[1.0; 256 * 4], wgpu::BufferUsages::UNIFORM);
    let atmosphere = gpu.buffer(&[0.0; 32], wgpu::BufferUsages::UNIFORM);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(&sprites),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 20,
            resource: light.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 21,
            resource: atmosphere.as_entire_binding(),
        },
    ];
    gpu.render_with_state(
        &source,
        "minified_item_vertex",
        &[
            Draw {
                fragment: "item_fragment",
                vertices: 0..3,
                bindings: &bindings,
                blend: None,
                write_depth: true,
            },
            Draw {
                fragment: "item_fragment",
                vertices: 3..6,
                bindings: &bindings,
                blend: None,
                write_depth: true,
            },
        ],
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
fn minified_dropped_item_transparent_texels_leave_colour_and_depth_uncovered() {
    let Some(gpu) = Gpu::for_fixture(
        "minified_dropped_item_transparent_texels_leave_colour_and_depth_uncovered",
    ) else {
        return;
    };
    let side = SNAPSHOT_SIDE as usize;
    for tint_alpha in [1.0, 0.5, 0.0] {
        let one = minified_item_raster(&gpu, 1, tint_alpha);
        let four = minified_item_raster(&gpu, 4, tint_alpha);
        for row in 0..side {
            for column in 0..side / 4 {
                let index = (row * side + column) * 4;
                assert_eq!(&one[index..index + 3], &[0, 255, 0]);
                assert_eq!(
                    &four[index..index + 3],
                    &[0, 255, 0],
                    "zero-alpha sprite texels must leave all background colour and depth samples available"
                );
            }
        }
        let pixel = |frame: &[u8], column: usize| {
            let index = (side / 2 * side + column) * 4;
            <[u8; 3]>::try_from(&frame[index..index + 3]).unwrap()
        };
        assert_eq!(
            pixel(&one, side * 3 / 8),
            [0, 255, 0],
            "1x retains the item cutoff below threshold"
        );
        if tint_alpha > 0.0 {
            assert_eq!(
                pixel(&one, side * 5 / 8),
                [255, 0, 0],
                "1x retains the item cutoff above threshold"
            );
            assert_eq!(
                pixel(&four, side * 7 / 8),
                [255, 0, 0],
                "opaque minified texels retain full coverage"
            );
            let partial = pixel(&four, side * 5 / 8);
            assert!(
                partial[0] > 0 && partial[1] > 0,
                "nonzero mip alpha still produces partial coverage"
            );
        } else {
            assert!(four.chunks_exact(4).all(|pixel| pixel[..3] == [0, 255, 0]));
        }
    }
}
