//! Pixel regressions for the texture path used by bounded bamboo models.
use crate::{
    gpu_snapshot::{Draw, Gpu},
    material_shader, shader_source,
};

/// Distinguishable edge texels and mip colours expose wrapping and excess minification.
fn atlas(gpu: &Gpu) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bounded model texture regression"),
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 5,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for level in 0..5 {
        let size = 16 >> level;
        let bytes: Vec<u8> = (0..size * size)
            .flat_map(|pixel| {
                let red = if level == 0 {
                    if pixel % size == 0 { 32 } else { 224 }
                } else {
                    [0, 64, 96, 128, 240][level as usize]
                };
                [red, 80, 40, 255]
            })
            .collect();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

#[test]
fn bamboo_tile_edges_and_distant_mips_preserve_the_admitted_rectangle() {
    let Some(gpu) = Gpu::for_fixture("bamboo texture edges and mip range") else {
        return;
    };
    let view = atlas(&gpu);
    let sampler = gpu
        .device
        .create_sampler(&material_shader::native_leaf_sampler_descriptor());
    let source = format!(
        "{}\n{FIXTURE}",
        shader_source::standalone(include_str!("../../src/model.wgsl"), &[])
    );
    let pixels = gpu.render(
        &source,
        "edge_vertex",
        &[Draw {
            fragment: "edge_fragment",
            vertices: 0..3,
            bindings: &[
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
            blend: None,
            write_depth: true,
        }],
    );
    for (column, expected) in [32_u8, 224, 128, 112].into_iter().enumerate() {
        let offset = (128 * 256 + column * 64 + 32) * 4;
        assert!(
            pixels[offset].abs_diff(expected) <= 1,
            "case {column}: sampled {}, expected {expected}",
            pixels[offset]
        );
        assert_eq!(pixels[offset + 3], 255);
    }
}

const FIXTURE: &str = r#"
@vertex fn edge_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let points = array(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    return vec4(points[index], 0.5, 1.0);
}
@fragment fn edge_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = min(u32(position.x) / 64u, 3u);
    let u = array(-0.001, 1.0, 0.5, 0.5)[index];
    // The last case blends point-sampled mip2 and mip3, without bilinear texel filtering.
    let gradient = array(0.0, 0.0, 8.0, exp2(2.5) / 16.0)[index];
    return sample_ref(0u, vec2(u, 0.5), vec2(gradient, 0.0), vec2(0.0, gradient));
}
"#;
