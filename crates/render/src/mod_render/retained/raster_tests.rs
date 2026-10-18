use crate::gpu_snapshot::{self, Draw, Gpu, SNAPSHOT_SIDE};
use bevy::prelude::{Mat4, Vec3};
use mod_render::retained::*;
use render_scene::mod_scene::retained::GpuInstance;

/// Renders an admitted line through the production vertex and fragment shaders.
fn line(gpu: &Gpu, positions: &[[f32; 3]], join: f32, blend: wgpu::BlendState) -> Vec<u8> {
    let mut store = Store::default();
    let handle = store
        .create_geometry(GeometrySpec::Polyline {
            vertices: positions
                .iter()
                .map(|&position| Vertex {
                    position,
                    color: [1., 1., 1., 0.5],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .collect(),
            closed: false,
        })
        .unwrap();
    store.commit();
    let vertices = mesh::build(&store.snapshot().geometry[&handle]);
    let view = gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO);
    let view = gpu.buffer(&view, wgpu::BufferUsages::UNIFORM);
    let storage = gpu.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
    let instance = GpuInstance {
        transform: Mat4::IDENTITY.to_cols_array_2d(),
        tint: [1.; 4],
        line: [20., 1., join, 1.],
    };
    let instances = gpu.buffer(
        bytemuck::cast_slice(&[instance]),
        wgpu::BufferUsages::STORAGE,
    );
    let source = crate::shader_source::standalone(include_str!("drawing.wesl"), &[]);
    gpu.render(
        &source,
        "vertex",
        &[Draw {
            fragment: "fragment",
            vertices: 0..vertices.len() as u32,
            bindings: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: storage.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: instances.as_entire_binding(),
                },
            ],
            blend: Some(blend),
            write_depth: false,
        }],
    )
}

/// Returns one RGBA pixel from the fixed-size headless target.
fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    &pixels[(y * SNAPSHOT_SIDE as usize + x) * 4..][..4]
}

#[test]
fn polyline_joins_cover_each_sample_once_for_alpha_and_additive_blending() {
    let Some(gpu) = Gpu::for_fixture("retained polyline coverage") else {
        return;
    };
    let additive = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::OVER,
    };
    for blend in [wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING, additive] {
        for join in [0., 1., 2.] {
            for [outgoing_x, outgoing_y] in [[0., 1.], [-0.4, 1.], [0.4, 1.], [-0.7, 0.25]] {
                for sign in [1., -1.] {
                    let pixels = line(
                        &gpu,
                        &[
                            [-0.5, -0.5 * sign, 0.5],
                            [0., -0.5 * sign, 0.5],
                            [outgoing_x, (-0.5 + outgoing_y) * sign, 0.5],
                        ],
                        join,
                        blend,
                    );
                    if join == 0.
                        && outgoing_x == 0.
                        && sign > 0.
                        && blend == wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
                    {
                        gpu_snapshot::save("retained-polyline-join", &pixels);
                    }
                    let expected = pixel(&pixels, 90, if sign > 0. { 192 } else { 64 })[0];
                    assert!(expected > pixel(&pixels, 0, 0)[0]);
                    assert!(
                        pixels.chunks_exact(4).all(|p| p[0] <= expected + 1),
                        "a join blended twice: join={join}, outgoing={outgoing_x}, sign={sign}"
                    );
                    if outgoing_x == 0. {
                        assert_eq!(
                            pixel(&pixels, 123, if sign > 0. { 188 } else { 68 })[0],
                            expected
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn duplicate_endpoints_and_corners_render_the_same_caps_and_joins() {
    let Some(gpu) = Gpu::for_fixture("retained polyline duplicate points") else {
        return;
    };
    let a = [-0.5, -0.5, 0.5];
    let b = [0., -0.5, 0.5];
    let c = [0., 0.5, 0.5];
    let blend = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
    for join in [0., 1., 2.] {
        let expected = line(&gpu, &[a, b, c], join, blend);
        for points in [[a, a, b, c], [a, b, b, c], [a, b, c, c]] {
            assert!(
                line(&gpu, &points, join, blend) == expected,
                "duplicate point changed the stroke"
            );
        }
    }
}
