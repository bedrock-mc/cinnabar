use crate::gpu_snapshot;
use crate::shader_source;

use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, DrawPipeline, Gpu};
use render::{
    BLOCK_ENTITY_VERTEX_WORDS, BLOCK_SELECTION_VERTICES_PER_EDGE, BlockEntityVertex,
    BlockSelectionFrame, BlockSelectionTarget, CrackShape,
};

fn source() -> String {
    shader_source::standalone(
        include_str!("../../src/block_entity/block_entity.wgsl"),
        &[],
    )
    .replace(
        "BLOCK_ENTITY_VERTEX_WORDS",
        &format!("{BLOCK_ENTITY_VERTEX_WORDS}u"),
    )
    .replace(
        "BLOCK_SELECTION_VERTICES_PER_EDGE",
        &format!("{BLOCK_SELECTION_VERTICES_PER_EDGE}u"),
    )
    // Lighting moves past the shader's own portal binding.
    .replace("@group(1) @binding(0)", "@group(0) @binding(5)")
    .replace("@group(1) @binding(1)", "@group(0) @binding(6)")
}

#[test]
fn selection_pixels_show_black_edges_or_a_brighter_surface() {
    let Some(gpu) = Gpu::for_fixture("block selection lines") else {
        return;
    };
    let target = BlockSelectionTarget {
        block: [0; 3],
        bounds: [[0.0; 3], [1.0; 3]],
        shape: CrackShape::Cube,
    };
    let eye = Vec3::new(2.0, 2.0, 4.0);
    let mut frame = BlockSelectionFrame::default();
    frame.update(Some(&target), false);
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
    let lightmap = gpu.buffer(&[1.0; 256 * 4], wgpu::BufferUsages::UNIFORM);
    let source = source();
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
        frame.update(Some(&target), mode == "outline");
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
            wgpu::BindGroupEntry {
                binding: 5,
                resource: lightmap.as_entire_binding(),
            },
        ];
        let mut draws = vec![Draw {
            fragment: "block_entity_overlay",
            vertices: 0..base.len() as u32,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }];
        let mut pipelines = vec![DrawPipeline {
            vertex: "block_entity_vertex",
            topology: wgpu::PrimitiveTopology::TriangleList,
        }];
        if mode != "before" {
            draws.push(Draw {
                fragment: if mode == "outline" {
                    "selection_line_fragment"
                } else {
                    "block_entity_crack"
                },
                vertices: if mode == "outline" {
                    base.len() as u32 / 2 * BLOCK_SELECTION_VERTICES_PER_EDGE
                        ..vertices.len() as u32 / 2 * BLOCK_SELECTION_VERTICES_PER_EDGE
                } else {
                    base.len() as u32..vertices.len() as u32
                },
                bindings: if mode == "outline" {
                    &bindings[..2]
                } else {
                    &bindings
                },
                blend: if mode == "outline" { None } else { Some(blend) },
                write_depth: mode == "outline",
            });
            pipelines.push(DrawPipeline {
                vertex: if mode == "outline" {
                    "selection_line_vertex"
                } else {
                    "block_entity_vertex"
                },
                topology: wgpu::PrimitiveTopology::TriangleList,
            });
        }
        let pixels = gpu.render_mixed(&source, &draws, &pipelines);
        gpu_snapshot::save(&format!("selection-{mode}"), &pixels);
        images.push(pixels);
    }
    let black = images[1]
        .as_chunks::<4>()
        .0
        .iter()
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

#[test]
fn selection_strokes_keep_visible_pixel_coverage_across_angles_and_depths() {
    let Some(gpu) = Gpu::for_fixture("block selection pixel coverage") else {
        return;
    };
    let source = source();
    let uniform = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    for z in [0.2, 0.8] {
        for end in [[0.0, 0.5, z], [0.5, 0.0, z], [0.5, 0.5, z]] {
            let start = [-end[0], -end[1], z];
            let vertices = [start, end].map(|position| BlockEntityVertex {
                position,
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            });
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
            ];
            let pixels = gpu.render(
                &source,
                "selection_line_vertex",
                &[Draw {
                    fragment: "selection_line_fragment",
                    vertices: 0..BLOCK_SELECTION_VERTICES_PER_EDGE,
                    bindings: &bindings,
                    blend: None,
                    write_depth: true,
                }],
            );
            let black = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[..3] == [0; 3])
                .count();
            assert!(
                black >= 240,
                "stroke coverage must survive rasterization: {end:?}, {black}"
            );
        }
    }
}
