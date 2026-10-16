//! Transparent cosmetic plates must leave the composited world untouched.
use crate::{gpu_snapshot, nametag_shader, shader_source};
use bevy::math::{Mat4, Vec3};
use render_model::NametagRecord;

#[test]
fn transparent_nametag_plate_preserves_world_color_and_alpha() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture("transparent nametag plate") else {
        return;
    };
    let shader =
        nametag_shader::from_wesl(include_str!("../../src/nametag.wesl"), "nametag fixture");
    let source = shader_source::standalone(shader.source.as_str(), &[]);
    let uniform = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let plate = NametagRecord {
        anchor: [0., 0., 0.5],
        rect: [-20., -10., 20., 10.],
        uv: [0., 0., -1., -1.],
        color: [0.; 4],
        ..Default::default()
    };
    let text = NametagRecord {
        text: 1,
        rect: [-16., -8., 16., 8.],
        uv: [0., 0., 1., 1.],
        color: [1., 0.9, 0.2, 1.],
        ..plate
    };
    let shadow = NametagRecord {
        rect: [-15., -7., 17., 9.],
        color: [0., 0., 0., 0.8],
        ..text
    };
    let records = gpu.buffer(
        bytemuck::cast_slice(&[plate, shadow, text]),
        wgpu::BufferUsages::STORAGE,
    );
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("original HI glyph fixture"),
        size: wgpu::Extent3d {
            width: 16,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut glyphs = vec![0; 16 * 8 * 4];
    for y in 1..7 {
        for x in 0..16 {
            if x == 1
                || x == 5
                || y == 4 && (1..=5).contains(&x)
                || x == 11
                || (y == 1 || y == 6) && (9..=13).contains(&x)
            {
                glyphs[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].fill(255);
            }
        }
    }
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &glyphs,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(16 * 4),
            rows_per_image: Some(8),
        },
        texture.size(),
    );
    let atlas = texture.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&Default::default());
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: records.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(&atlas),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
    ];
    let pixels = gpu.render(
        &source,
        "nametag_vertex",
        &[gpu_snapshot::Draw {
            fragment: "nametag_fragment",
            vertices: 0..18,
            bindings: &bindings,
            blend: Some(wgpu::BlendState {
                alpha: wgpu::BlendComponent::REPLACE,
                ..wgpu::BlendState::ALPHA_BLENDING
            }),
            write_depth: false,
        }],
    );
    gpu_snapshot::save("transparent-nametag-plate", &pixels);
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] > 200 && pixel[1] > 200),
        "text remains visible"
    );
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] == 204),
        "the nonzero shadow opacity remains visible"
    );
    let corner = &pixels[..4];
    // Inside the plate but outside the text and its one-pixel shadow.
    let center =
        (gpu_snapshot::SNAPSHOT_SIDE * (gpu_snapshot::SNAPSHOT_SIDE / 2) + 65) as usize * 4;
    assert_eq!(
        &pixels[center..center + 4],
        corner,
        "a disabled background must preserve world RGBA, including compositor alpha"
    );
}
