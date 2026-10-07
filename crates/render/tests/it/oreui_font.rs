//! Native font coverage preserves its text-colour gamma and raster sampling.

use crate::{
    gpu_snapshot::{Draw, Gpu},
    ui_shader,
};

#[test]
fn oreui_font_edges_use_native_coverage_gamma_sampling_and_sdf_scale() {
    let Some(gpu) = Gpu::for_fixture("OreUI native text coverage") else {
        return;
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("font edge coverage"),
        size: wgpu::Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[128, 0],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(2),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let page = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let viewport = gpu.buffer(&[256.0, 256.0, 0.0, 0.0], wgpu::BufferUsages::UNIFORM);
    let format = gpu.words(&[1, 0, 0, 0], wgpu::BufferUsages::UNIFORM);
    let nearest = gpu
        .device
        .create_sampler(&wgpu::SamplerDescriptor::default());
    let linear = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: viewport.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(&page),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::Sampler(&nearest),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(&linear),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: format.as_entire_binding(),
        },
    ];
    for (color, exponent) in [
        ([1.0, 1.0, 1.0], 0.45f32),
        ([0.0, 0.0, 0.0], 1.45),
        ([1.0, 0.0, 0.0], 1.2374),
    ] {
        let source = format!(
            "{}\n{}",
            ui_shader::source(include_str!("../../src/ui.wgsl")),
            vertex(color, u32::from(assets::FONT_STYLE_COVERAGE_GAMMA) | 8, 0.0)
        );
        let pixels = gpu.render(
            &source,
            "font_fixture_vertex",
            &[Draw {
                fragment: "ui_fragment",
                vertices: 0..3,
                bindings: &bindings,
                blend: None,
                write_depth: false,
            }],
        );
        let actual = &pixels[(128 * 256 + 128) * 4..][..4];
        let coverage = (128.0f32 / 255.0).powf(exponent);
        let expected = [
            color[0] * coverage,
            color[1] * coverage,
            color[2] * coverage,
            coverage,
        ]
        .map(|c| (c * 255.0).round() as u8);
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                actual.abs_diff(expected) <= 1,
                "native edge: actual {actual}, expected {expected}"
            );
        }
    }
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[132, 132],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(2),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let flags = u32::from(assets::FONT_STYLE_COVERAGE_GAMMA | assets::FONT_STYLE_SDF);
    for texels_per_pixel in [0.5, 1.0, 2.0, 4.0] {
        let source = format!(
            "{}\n{}",
            ui_shader::source(include_str!("../../src/ui.wgsl")),
            vertex([1.0; 3], flags, texels_per_pixel)
        );
        let pixels = gpu.render(
            &source,
            "font_fixture_vertex",
            &[Draw {
                fragment: "ui_fragment",
                vertices: 0..3,
                bindings: &bindings,
                blend: None,
                write_depth: false,
            }],
        );
        let actual = &pixels[(128 * 256 + 128) * 4..][..4];
        let threshold = (128.0 / 255.0) * texels_per_pixel;
        let distance = (132.0 / 255.0) * 7.96875 - 3.984375;
        let t = ((distance + threshold) / (2.0 * threshold)).clamp(0.0, 1.0);
        let coverage = (t * t * (3.0 - 2.0 * t)).powf(0.45);
        let expected = (coverage * 255.0).round() as u8;
        assert!(
            actual
                .iter()
                .all(|&channel| channel.abs_diff(expected) <= 1),
            "native SDF scale {texels_per_pixel}: {actual:?}, expected {expected}"
        );
    }
}

fn vertex(color: [f32; 3], flags: u32, texels_per_pixel: f32) -> String {
    let uv = if texels_per_pixel == 0.0 {
        "vec2(0.9, 0.5)".to_owned()
    } else {
        format!(
            "vec2({}, {}) + points[i] * {}",
            0.5 - 0.5 * texels_per_pixel,
            0.5 + 0.5 * texels_per_pixel,
            128.0 * texels_per_pixel
        )
    };
    format!(
        r#"
@vertex fn font_fixture_vertex(@builtin(vertex_index) i: u32) -> UiVertexOutput {{
    let points = array<vec2<f32>, 3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    var output: UiVertexOutput;
    output.clip_position = vec4(points[i], 1.0, 1.0);
    output.uv = {uv};
    output.color = vec4({r}, {g}, {b}, 1.0);
    output.texture_page = 0u;
    output.style_flags = {flags}u;
    output.alpha_cutoff = -1.0;
    output.model_light = 1.0;
    output.overlay_color = vec4(0.0);
    return output;
}}
"#,
        r = color[0],
        g = color[1],
        b = color[2]
    )
}
