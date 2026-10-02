#[path = "support/gpu_snapshot.rs"]
mod gpu_snapshot;
#[path = "support/shader_source.rs"]
mod shader_source;

use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu};
use render::{BlockSelectionFrame, BlockSelectionTarget, CrackShape};

#[test]
fn selection_pixels_show_black_edges_or_a_brighter_surface() {
    let Some(gpu) = Gpu::new() else { return };
    let target = BlockSelectionTarget {
        block: [0; 3],
        bounds: [[0.0; 3], [1.0; 3]],
        shape: CrackShape::Cube,
    };
    let eye = Vec3::new(2.0, 2.0, 4.0);
    let forward = (Vec3::splat(0.5) - eye).normalize();
    let mut frame = BlockSelectionFrame::default();
    frame.update(Some(&target), eye, forward, false);
    let base = frame
        .highlight
        .iter()
        .map(|vertex| {
            let mut vertex = *vertex;
            vertex.position = vertex.position.map(|v| (v - 0.5) / 1.004 + 0.5);
            vertex.color = [0.24, 0.42, 0.21, 1.0];
            vertex
        })
        .collect::<Vec<_>>();
    let matrix = Mat4::perspective_infinite_reverse_rh(0.7, 1.0, 0.1)
        * Mat4::look_at_rh(eye, Vec3::splat(0.5), Vec3::Y);
    let uniform = gpu.buffer(
        &gpu_snapshot::view(matrix, eye),
        wgpu::BufferUsages::UNIFORM,
    );
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let texture_view = texture.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&Default::default());
    let source =
        shader_source::standalone(include_str!("../src/block_entity/block_entity.wgsl"), &[]);
    let blend = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,
            dst_factor: wgpu::BlendFactor::Src,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    };
    let mut images = Vec::new();
    for mode in ["before", "outline", "highlight"] {
        frame.update(Some(&target), eye, forward, mode == "outline");
        let overlay = if mode == "outline" {
            &frame.outline
        } else {
            &frame.highlight
        };
        let vertices = base
            .iter()
            .chain(overlay.iter())
            .copied()
            .collect::<Vec<_>>();
        let buffer = gpu.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
        let bindings = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ];
        let mut draws = vec![Draw {
            fragment: "block_entity_overlay",
            vertices: 0..base.len() as u32,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }];
        if mode != "before" {
            draws.push(Draw {
                fragment: if mode == "outline" {
                    "block_entity_overlay"
                } else {
                    "block_entity_crack"
                },
                vertices: base.len() as u32..vertices.len() as u32,
                bindings: &bindings,
                blend: Some(if mode == "outline" {
                    wgpu::BlendState::ALPHA_BLENDING
                } else {
                    blend
                }),
                write_depth: false,
            });
        }
        let pixels = gpu.render(&source, "block_entity_vertex", &draws);
        gpu_snapshot::save(&format!("selection-{mode}"), &pixels);
        images.push(pixels);
    }
    let black = images[1]
        .chunks_exact(4)
        .filter(|p| p[..3] == [0; 3])
        .count();
    assert!(
        black > 100,
        "visible outline edges must survive depth testing: {black}"
    );
    let center = (128 * 256 + 128) * 4;
    assert!(
        images[2][center + 1] > images[0][center + 1] + 20,
        "highlight must brighten the backing block"
    );
}
