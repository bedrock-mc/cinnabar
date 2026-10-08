//! Native liquid winding, using real packed faces and the production fragments.
use crate::chunk_constants;
use crate::gpu_snapshot;
use crate::shader_source;

use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu, RasterState};
use meshing::liquid::{LIQUID_DEPTH_WRITE_BIT, LIQUID_TOP_INSET_BIT, LIQUID_TWO_SIDED_BIT};
use meshing::{Face, PackedLiquidQuad};

#[test]
fn native_liquid_faces_admit_only_their_original_and_marked_reverse_windings() {
    let Some(gpu) = Gpu::for_fixture("liquid_raster") else {
        return;
    };
    let indices = chunk_constants::STATIC_QUAD_INDICES;
    let index_source = format!(
        "const RASTER_INDICES: array<u32, {}> = array({});",
        indices.len(),
        indices.map(|index| format!("{index}u")).join(", ")
    );
    let source = format!(
        "{}\n{index_source}\n{WITNESS}",
        shader_source::standalone(
            include_str!("../../src/liquid.wgsl"),
            &["NATIVE_GAMMA_BLEND"]
        )
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(20)");
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("liquid winding white-texel witness"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
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
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let atlas = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&Default::default());
    let frame = render::AtmosphereFrame::default();
    assert!(
        frame.fog_start() > 3.0,
        "fixture must remain outside distance fog"
    );
    let atmosphere = gpu.buffer(
        bytemuck::cast_slice(std::slice::from_ref(&frame)),
        wgpu::BufferUsages::UNIFORM,
    );
    let table = render::LightmapInputs::default()
        .build()
        .map(|_| [1.0_f32; 4]);
    let lightmap = gpu.buffer(bytemuck::cast_slice(&table), wgpu::BufferUsages::UNIFORM);
    let mut failures = Vec::new();
    for face in Face::ALL {
        // The ordinary top duplicates its reverse; sides do so only
        // beside primary air. Vanilla never duplicates the bottom.
        let side_admission: &[bool] = match face {
            Face::NegativeY => &[false],
            Face::PositiveY => &[true],
            _ => &[false, true],
        };
        for &two_sided in side_admission {
            for depth_route in [false, true] {
                let heights = match face {
                    Face::NegativeY => [0; 4],
                    Face::PositiveY => [255; 4],
                    _ => [0, 255, 255, 0],
                };
                let mut words =
                    PackedLiquidQuad::try_pack([0; 3], face, heights, 0, 0, [0; 2], false)
                        .unwrap()
                        .words();
                if face != Face::NegativeY {
                    words[2] |= LIQUID_TOP_INSET_BIT;
                }
                if two_sided {
                    words[2] |= LIQUID_TWO_SIDED_BIT;
                }
                if depth_route {
                    words[2] |= LIQUID_DEPTH_WRITE_BIT;
                }
                let case = gpu.buffer(bytemuck::cast_slice(&words), wgpu::BufferUsages::STORAGE);
                for inside in [false, true] {
                    let (matrix, eye) = face_view(face, inside);
                    let view = gpu.buffer(
                        &gpu_snapshot::view(matrix, eye),
                        wgpu::BufferUsages::UNIFORM,
                    );
                    let bindings = [
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: view.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                            resource: wgpu::BindingResource::TextureView(&atlas),
                        },
                        wgpu::BindGroupEntry {
                            binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                            resource: wgpu::BindingResource::TextureView(&atlas),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 15,
                            resource: atmosphere.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 20,
                            resource: lightmap.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 24,
                            resource: case.as_entire_binding(),
                        },
                    ];
                    for (fragment, fragment_depth_route) in
                        [("fragment", false), ("fragment_depth", true)]
                    {
                        let pixels = gpu.render_with_state(
                            &source,
                            "raster_witness",
                            &[Draw {
                                fragment,
                                vertices: 0..indices.len() as u32,
                                bindings: &bindings,
                                blend: (!depth_route).then_some(wgpu::BlendState::ALPHA_BLENDING),
                                write_depth: true,
                            }],
                            RasterState {
                                primitive: wgpu::PrimitiveState {
                                    front_face: wgpu::FrontFace::Cw,
                                    cull_mode: None,
                                    ..Default::default()
                                },
                                write_mask: wgpu::ColorWrites::RED
                                    | wgpu::ColorWrites::GREEN
                                    | wgpu::ColorWrites::BLUE,
                                ..Default::default()
                            },
                        );
                        let side = (pixels.len() / 4).isqrt();
                        let actual = &pixels[(side / 2 * side + side / 2) * 4..][..4];
                        let visible = depth_route == fragment_depth_route && (!inside || two_sided);
                        let expected: [u8; 3] = [0.12_f32, 0.18, 0.25].map(|receiver| {
                            let colour = if !visible {
                                receiver
                            } else if depth_route {
                                1.0
                            } else {
                                0.5 + 0.5 * receiver
                            };
                            (colour * 255.0).round() as u8
                        });
                        if actual[..3]
                            .iter()
                            .zip(expected)
                            .any(|(&actual, expected)| actual.abs_diff(expected) > 1)
                            || actual[3] != 255
                        {
                            failures.push(format!("{face:?}, inside {inside}, two_sided {two_sided}, depth_route {depth_route}, {fragment}: {actual:?} != {expected:?}"));
                        }
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn face_view(face: Face, inside: bool) -> (Mat4, Vec3) {
    let normal = match face {
        Face::NegativeX => -Vec3::X,
        Face::PositiveX => Vec3::X,
        Face::NegativeY => -Vec3::Y,
        Face::PositiveY => Vec3::Y,
        Face::NegativeZ => -Vec3::Z,
        Face::PositiveZ => Vec3::Z,
    };
    let centre = Vec3::splat(0.5) + normal * 0.5;
    let eye = centre + normal * if inside { -2.0 } else { 2.0 };
    let up = if normal.y == 0.0 { Vec3::Y } else { Vec3::Z };
    let projection = Mat4::perspective_infinite_reverse_rh(std::f32::consts::FRAC_PI_4, 1.0, 0.1);
    (projection * Mat4::look_at_rh(eye, centre, up), eye)
}

// Vertex positions come from production liquid_corner, not a screen-space
// winding proxy. The same production index order feeds both actual triangles.
const WITNESS: &str = r#"
@group(0) @binding(24) var<storage, read> raster_case: array<vec4<u32>>;
@vertex fn raster_witness(@builtin(vertex_index) index: u32) -> VertexOutput {
    let data = raster_case[0];
    let world = liquid_corner(data.x, data.y, RASTER_INDICES[index], data.z);
    var out: VertexOutput;
    out.clip_position = view.clip_from_world * vec4(world, 1.0);
    out.world_position = world;
    out.uv = vec2(0.5);
    out.current_texture = 0u;
    out.next_texture = 0u;
    out.frame_blend = 0.0;
    out.water_tint = vec4(1.0, 1.0, 1.0, 0.5);
    out.lighting = vec3(1.0);
    out.native_light_levels = vec2(0.0);
    out.native_face_shade = 1.0;
    out.depth_write_route = u32((data.z & LIQUID_DEPTH_WRITE_BIT) != 0u);
    out.two_sided = u32((data.z & LIQUID_TWO_SIDED_BIT) != 0u);
    return out;
}
"#;
