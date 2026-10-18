use crate::gpu_snapshot::{self, Draw, Gpu, SNAPSHOT_SIDE};
use bevy::prelude::{Mat4, Vec3};
use mod_render::retained::*;
use render_scene::mod_scene::retained::GpuInstance;

/// Renders an admitted line through the production vertex and fragment shaders.
fn line(gpu: &Gpu, positions: &[[f32; 3]], join: f32, blend: wgpu::BlendState) -> Vec<u8> {
    transformed_line(gpu, positions, Mat4::IDENTITY, 20., join, blend)
}

/// Renders local geometry with the same placement transform used by retained instances.
fn transformed_line(
    gpu: &Gpu,
    positions: &[[f32; 3]],
    transform: Mat4,
    width: f32,
    join: f32,
    blend: wgpu::BlendState,
) -> Vec<u8> {
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
    let mut vertices = mesh::build(&store.snapshot().geometry[&handle]);
    let vertex_count = vertices.len() as u32;
    if vertices.is_empty() {
        vertices.push(mesh::GpuVertex::default());
    }
    let frame = super::gpu::CameraFrame {
        clip: Mat4::IDENTITY.to_cols_array_2d(),
        viewport: [SNAPSHOT_SIDE as f32, SNAPSHOT_SIDE as f32, 1., 0.],
        animation: [0.; 4],
    };
    let view = gpu.buffer(bytemuck::cast_slice(&[frame]), wgpu::BufferUsages::UNIFORM);
    let storage = gpu.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
    let instance = GpuInstance {
        transform: transform.to_cols_array_2d(),
        tint: [1.; 4],
        line: [width, 1., join, 1.],
        params: mod_api::materials::Preset::default().0,
    };
    let instances = gpu.buffer(
        bytemuck::cast_slice(&[instance]),
        wgpu::BufferUsages::STORAGE,
    );
    let source = super::materials::source(
        &mod_render::material::module(mod_render::material::BUILTIN),
        include_str!("drawing.wgsl"),
    );
    gpu.render(
        &source,
        "vertex",
        &[Draw {
            fragment: "material_fragment",
            vertices: 0..vertex_count,
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
fn scaled_small_polyline_renders_the_same_stroke_as_world_sized_geometry() {
    let Some(gpu) = Gpu::for_fixture("retained scaled polyline") else {
        return;
    };
    let blend = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
    let expected = line(&gpu, &[[-0.5, 0., 0.5], [0.5, 0., 0.5]], 0., blend);
    assert_ne!(pixel(&expected, 128, 128), pixel(&expected, 0, 0));
    let pixels = transformed_line(
        &gpu,
        &[[0.; 3], [1e-8, 0., 0.]],
        Mat4::from_translation(Vec3::new(-0.5, 0., 0.5)) * Mat4::from_scale(Vec3::new(1e8, 1., 1.)),
        20.,
        0.,
        blend,
    );
    gpu_snapshot::save("retained-scaled-polyline", &pixels);
    assert!(pixels == expected, "instance scaling changed the stroke");
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
                        pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|p| p[0] <= expected + 1),
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
fn consecutive_short_segment_joins_cover_each_sample_once() {
    let Some(gpu) = Gpu::for_fixture("retained consecutive short joins") else {
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
            for sign in [1., -1.] {
                // Pixel-space legs are 100, 10 and 100 pixels long with a 20-pixel radius.
                let positions = [[-100., 0.], [0., 0.], [0., 10.], [100., 10.]]
                    .map(|[x, y]| [x / 128., y * sign / 128., 0.5]);
                let pixels = transformed_line(&gpu, &positions, Mat4::IDENTITY, 40., join, blend);
                if join == 0.
                    && sign > 0.
                    && blend == wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
                {
                    gpu_snapshot::save("retained-consecutive-short-joins", &pixels);
                }
                let expected = pixel(&pixels, 70, 128)[0];
                assert!(
                    expected > pixel(&pixels, 0, 0)[0],
                    "stroke must remain visible"
                );
                assert!(
                    pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[0] <= expected + 1),
                    "consecutive joins blended twice: join={join}, sign={sign}, blend={blend:?}"
                );
                assert_eq!(
                    pixel(&pixels, 128, if sign > 0. { 122 } else { 133 })[0],
                    expected
                );
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

#[test]
fn short_reversing_polylines_keep_joins_within_the_stroke_bounds() {
    let Some(gpu) = Gpu::for_fixture("retained polyline reversal bounds") else {
        return;
    };
    let blend = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
    for join in [0., 1., 2.] {
        for sign in [1., -1.] {
            for end in [[-0.5, 0.01 * sign, 0.5], [-0.1, 0.002 * sign, 0.5]] {
                let pixels = line(&gpu, &[[-0.5, 0., 0.5], [0., 0., 0.5], end], join, blend);
                if join == 0. && sign > 0. && end[0] == -0.5 {
                    gpu_snapshot::save("retained-polyline-reversal", &pixels);
                }
                let clear = pixel(&pixels, 0, 0);
                let max_x = if join == 1. { 168 } else { 138 };
                assert_ne!(
                    pixel(&pixels, 100, 128),
                    clear,
                    "stroke must remain visible"
                );
                for y in 0..SNAPSHOT_SIDE as usize {
                    for x in 0..SNAPSHOT_SIDE as usize {
                        if !(54..=max_x).contains(&x) || !(116..=140).contains(&y) {
                            assert_eq!(
                                pixel(&pixels, x, y),
                                clear,
                                "join escaped the segment bounds: join={join}, sign={sign}, end={end:?}, pixel=({x},{y})"
                            );
                        }
                    }
                }
            }
        }
    }
}
