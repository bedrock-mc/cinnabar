use super::*;
use bevy::math::Vec3;

use crate::gpu_snapshot::{Draw, DrawPipeline, Gpu, RasterState, SNAPSHOT_SIDE};

/// Reads the actual specialized states, including the retained terrain-unit path.
fn depth_biases(units: bool, msaa: Msaa) -> [bevy::render::render_resource::DepthBiasState; 2] {
    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    let mut pipelines = ChunkPipeline::from_world(&mut World::new());
    let key = ChunkPipelineKey {
        msaa,
        hdr: false,
        enhanced: false,
    };
    [2, 1].map(|stream| {
        let variants = if units {
            &mut pipelines.unit_variants[stream]
        } else if stream == 2 {
            &mut pipelines.model_variants
        } else {
            &mut pipelines.variants
        };
        let id = variants.specialize(&cache, key).unwrap();
        crate::queue_review_support::queued_descriptor(&mut cache, id)
            .depth_stencil
            .as_ref()
            .unwrap()
            .bias
    })
}

#[test]
fn two_sided_cutout_keeps_exposed_pixels_without_fighting_a_partial_opaque_face() {
    let Some(gpu) = Gpu::for_fixture("cutout contact depth") else {
        return;
    };
    for units in [false, true] {
        for msaa in [Msaa::Off, Msaa::Sample4] {
            let biases = depth_biases(units, msaa);
            for frame in 0..12 {
                let angle = frame as f32 * 0.07 - 0.35;
                let eye = Vec3::new(1.5 + angle, 0.75 + angle * 0.25, 2.0);
                let clip =
                    glam::camera::rh::proj::directx::perspective_infinite_reverse(0.9, 1.0, 0.05)
                        * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
                let uniform = gpu.buffer(&clip.to_cols_array(), wgpu::BufferUsages::UNIFORM);
                let bindings = [wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }];
                let render = |leaf_first: bool| {
                    let order = if leaf_first { [1, 0] } else { [0, 1] };
                    let draws = order.map(|index| Draw {
                        fragment: if index == 0 { "stone" } else { "leaf" },
                        vertices: 0..6,
                        bindings: &bindings,
                        blend: None,
                        write_depth: true,
                    });
                    let pipelines = order.map(|index| DrawPipeline {
                        vertex: if index == 0 { "partial" } else { "cutout" },
                        topology: wgpu::PrimitiveTopology::TriangleList,
                    });
                    gpu.render_sources(
                        &[CONTACT_SHADER; 2],
                        &draws,
                        &pipelines,
                        RasterState {
                            multisample: wgpu::MultisampleState {
                                count: msaa.samples(),
                                ..default()
                            },
                            depth_biases: order.map(|index| biases[index]).to_vec(),
                            ..default()
                        },
                    )
                };
                let stone_first = render(false);
                let leaf_first = render(true);
                let different_pixels = stone_first
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(leaf_first.as_chunks::<4>().0.iter())
                    .filter(|(a, b)| a != b)
                    .count();
                assert_eq!(
                    different_pixels, 0,
                    "contact depends on draw order: units={units}, msaa={msaa:?}, frame={frame}"
                );
                let mut stone_pixels = 0;
                let mut leaf_pixels = 0;
                let mut hole_pixels = 0;
                let world_from_clip = clip.inverse();
                for (index, pixel) in stone_first.as_chunks::<4>().0.iter().enumerate() {
                    let x = index % SNAPSHOT_SIDE as usize;
                    let y = index / SNAPSHOT_SIDE as usize;
                    let ndc = Vec3::new(
                        (x as f32 + 0.5) * 2.0 / SNAPSHOT_SIDE as f32 - 1.0,
                        1.0 - (y as f32 + 0.5) * 2.0 / SNAPSHOT_SIDE as f32,
                        0.5,
                    );
                    let point = world_from_clip.project_point3(ndc);
                    let ray = (point - eye).normalize();
                    let on_face = eye + ray * (-eye.z / ray.z);
                    if on_face.x.abs() > 0.85 || on_face.y.abs() > 0.85 || on_face.y.abs() < 0.05 {
                        continue;
                    }
                    if on_face.y < 0.0 {
                        assert_eq!(
                            *pixel,
                            [255, 0, 0, 255],
                            "cutout overwrites the opaque half"
                        );
                        stone_pixels += 1;
                    } else if *pixel == [0, 255, 0, 255] {
                        leaf_pixels += 1;
                    } else if pixel[0] > 0 && pixel[1] > 0 && pixel[2] > 0 {
                        hole_pixels += 1;
                    }
                }
                assert!(
                    stone_pixels > 100 && leaf_pixels > 100 && hole_pixels > 100,
                    "the opaque contact, exposed cutout and alpha holes must all render"
                );
            }
        }
    }
}

// Opposite winding and independent triangulations reproduce a leaf's back face
// sharing the lower half of its plane with a stair or slab side. The upper half
// has alpha holes, which must remain cut out after the depth offset.
const CONTACT_SHADER: &str = r#"
@group(0) @binding(0) var<uniform> clip_from_world: mat4x4<f32>;
@vertex fn partial(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
    let corners = array(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 0.0), vec2(-1.0, 0.0));
    let indices = array(0u, 1u, 2u, 0u, 2u, 3u);
    return clip_from_world * vec4(corners[indices[id]], 0.0, 1.0);
}
@vertex fn cutout(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
    let corners = array(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    let indices = array(0u, 3u, 1u, 1u, 3u, 2u);
    return clip_from_world * vec4(corners[indices[id]], 0.0, 1.0);
}
@fragment fn stone() -> @location(0) vec4<f32> { return vec4(1.0, 0.0, 0.0, 1.0); }
@fragment fn leaf(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    if ((u32(p.x) / 8u + u32(p.y) / 8u) % 2u == 0u) { discard; }
    return vec4(0.0, 1.0, 0.0, 1.0);
}
"#;
