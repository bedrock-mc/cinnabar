//! Native cube face axes must not align opposite cutout masks in world space.
use crate::gpu_snapshot;

use gpu_snapshot::{Draw, Gpu};
use meshing::Face;
use render::greedy_texture_uv;

/// Compile the actual production functions, not a test-side UV implementation.
fn shader() -> String {
    let source = include_str!("../../src/chunk.wesl");
    let corners = source.split_once("fn quad_corner(").unwrap().1;
    let corners = corners.split_once("\nfn face_normal(").unwrap().0;
    let uv = source.split_once("fn greedy_uv(").unwrap().1;
    let uv = uv.split_once("\n@vertex").unwrap().0;
    format!("fn quad_corner({corners}\nfn greedy_uv({uv}\n{FIXTURE}")
}

#[test]
fn native_cube_axes_are_applied_before_all_pack_rotations_and_reflections() {
    // Current 1.26.50.26 cube tessellator: West/East and North/South reverse U;
    // Down reverses V relative to Up. These are coordinates at shared points,
    // not a comparison of differently ordered face vertices.
    let corners = [
        [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]],
        [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]],
        [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    ];
    for (face, expected) in Face::ALL.into_iter().zip(corners) {
        for flags in 0..16 {
            for (corner, [u, v]) in expected.into_iter().enumerate() {
                let (width, height) = (7.0, 3.0);
                let (u, v) = (u * width, v * height);
                let (mut expected, extents) = match flags & 3 {
                    1 => ([v, width - u], [height, width]),
                    2 => ([width - u, height - v], [width, height]),
                    3 => ([height - v, u], [height, width]),
                    _ => ([u, v], [width, height]),
                };
                for axis in 0..2 {
                    if flags & (4 << axis) != 0 {
                        expected[axis] = extents[axis] - expected[axis];
                    }
                }
                assert_eq!(
                    greedy_texture_uv(face, corner as u32, width as u32, height as u32, flags),
                    expected,
                    "{face:?} corner {corner}, flags {flags}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn actual_gpu_uvs_follow_native_world_axes_for_every_face() {
    let gpu = Gpu::new().expect("this fixture requires a native GPU adapter");
    let source = shader();
    for face in Face::ALL {
        let config = gpu.buffer(
            &[f32::from_bits(face as u32), 0.0, 0.0, 0.0],
            wgpu::BufferUsages::UNIFORM,
        );
        let pixels = gpu.render(
            &source,
            "leaf_vertex",
            &[Draw {
                fragment: "uv_fragment",
                vertices: 0..6,
                bindings: &[wgpu::BindGroupEntry {
                    binding: 2,
                    resource: config.as_entire_binding(),
                }],
                blend: None,
                write_depth: true,
            }],
        );
        let side = ((pixels.len() / 4) as f64).sqrt() as usize;
        for (x, y) in [(side / 8, side / 4), (side * 3 / 4, side * 5 / 8)] {
            let mut uv = [
                (x as f32 + 0.5) / side as f32,
                (y as f32 + 0.5) / side as f32,
            ];
            if matches!(face, Face::PositiveX | Face::NegativeZ) {
                uv[0] = 1.0 - uv[0];
            }
            if face == Face::PositiveY {
                uv[1] = 1.0 - uv[1];
            }
            let pixel = &pixels[(y * side + x) * 4..][..4];
            for channel in 0..2 {
                assert!(
                    (f32::from(pixel[channel]) - uv[channel] * 255.0).abs() <= 1.0,
                    "GPU {face:?} axis {channel}: {pixel:?} != {uv:?}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn actual_gpu_draws_mirror_opposing_leaf_alpha_masks_on_all_three_axes() {
    let gpu = Gpu::new().expect("this fixture requires a native GPU adapter");
    // An authored asymmetric mask: only its upper-left texel is transparent.
    // Rendering the same plane twice leaves one quarter uncovered; rendering
    // native opposing faces closes that artificial straight-through opening.
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("asymmetric leaf alpha witness"),
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8),
            rows_per_image: Some(2),
        },
        texture.size(),
    );
    let view = texture.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let source = shader();
    for faces in [[0, 1], [2, 3], [4, 5]] {
        let configs = faces.map(|face| {
            gpu.buffer(
                &[f32::from_bits(face), 0.0, 0.0, 0.0],
                wgpu::BufferUsages::UNIFORM,
            )
        });
        let bindings = configs.each_ref().map(|config| {
            [
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: config.as_entire_binding(),
                },
            ]
        });
        for (back, expected_covered) in [(0, 3), (1, 4)] {
            let pixels = gpu.render(
                &source,
                "leaf_vertex",
                &[
                    Draw {
                        fragment: "leaf_fragment",
                        vertices: 0..6,
                        bindings: &bindings[back],
                        blend: None,
                        write_depth: true,
                    },
                    Draw {
                        fragment: "leaf_fragment",
                        vertices: 0..6,
                        bindings: &bindings[0],
                        blend: None,
                        write_depth: true,
                    },
                ],
            );
            let covered = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[1] == 255)
                .count();
            assert_eq!(
                covered * 4,
                pixels.len() / 4 * expected_covered,
                "opposing faces {faces:?}, control={}",
                back == 0
            );
        }
    }
}

const FIXTURE: &str = r#"
@group(0) @binding(0) var mask_texture: texture_2d<f32>;
@group(0) @binding(1) var mask_sampler: sampler;
@group(0) @binding(2) var<uniform> face_config: vec4<u32>;
struct LeafVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
@vertex
fn leaf_vertex(@builtin(vertex_index) index: u32) -> LeafVertex {
    let corners = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    let corner = corners[index];
    let face = face_config.x;
    let point = quad_corner(face, corner, vec3(0.0), 1.0, 1.0);
    var projected = point.xy;
    if (face < 2u) { projected = point.zy; }
    if (face == 2u || face == 3u) { projected = point.xz; }
    var out: LeafVertex;
    out.position = vec4(projected * 2.0 - 1.0, 0.5, 1.0);
    out.uv = greedy_uv(face, quad_position(face, point), 1.0, 1.0, face_config.y);
    return out;
}
@fragment
fn leaf_fragment(in: LeafVertex) -> @location(0) vec4<f32> {
    if (textureSample(mask_texture, mask_sampler, in.uv).a < 0.5) { discard; }
    return vec4(0.0, 1.0, 0.0, 1.0);
}
@fragment
fn uv_fragment(in: LeafVertex) -> @location(0) vec4<f32> {
    return vec4(in.uv, 0.0, 1.0);
}
"#;
