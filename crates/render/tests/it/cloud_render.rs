#[path = "cloud_render/corners.rs"]
mod corners;
use crate::gpu_snapshot;
#[path = "cloud_render/raster.rs"]
mod raster;
use crate::shader_source;

use meshing::cloud_viewport::{CLOUD_FADE_START, shader_source as cloud_source};

fn shader() -> String {
    shader_source::standalone(&cloud_source(include_str!("../../src/cloud.wesl")), &[])
}

fn compact(source: &str) -> String {
    source.chars().filter(|ch| !ch.is_whitespace()).collect()
}

#[test]
fn native_cloud_shader_validates_and_vertex_pulls_one_signed_sampling_window() {
    let source = shader();
    let module = naga::front::wgsl::parse_str(&source).expect("parse native cloud WGSL");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate native cloud WGSL");
    assert!(source.contains("cell: vec2<i32>"));
    assert!(source.contains("let quad_index = vertex_index / 6u;"));
    assert!(source.contains("let corner_index = vertex_index % 6u;"));
    assert!(source.contains("cloud_records[quad_index]"));
    assert!(!source.contains("instance_index"));
    assert!(source.contains("atmosphere.fog_end_time.z * native_cloud.geometry.w"));
    assert!(source.contains("view.clip_from_world * vec4(world_position, 1.0)"));
    assert!(!source.contains("position.z = 0.0"));
}

#[test]
fn one_sorted_item_draws_exact_quad_vertices_and_one_window_instance() {
    let source = include_str!("../../src/cloud_render.rs");
    let compact_source = compact(source);
    assert!(source.contains("PhaseItemExtraIndex::None"));
    assert!(source.contains("cloud_phase_distance("));
    assert!(compact_source.contains("prepared.record_count.checked_mul(6)"));
    assert!(source.contains("pass.draw(0..vertex_count, 0..1);"));
    assert_eq!(source.matches("pass.draw(").count(), 1);
    assert!(!source.contains("0..9"));
    assert!(!source.contains("StandardMaterial"));
}

#[test]
fn cloud_colour_and_fade_are_native_vertex_work_not_reinvented_fragment_weather() {
    use gpu_snapshot::{Draw, Gpu};
    let Some(gpu) = Gpu::for_fixture("default cloud fragment colour and fade") else {
        return;
    };
    let mut atmosphere = crate::gpu_snapshot::atmosphere();
    atmosphere[11] = 1.0;
    atmosphere[15] = 1.0;
    atmosphere[23] = 1.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    for encoded in [false, true] {
        let mut source = shader_source::standalone(
            include_str!("../../src/cloud.wesl"),
            if encoded { &["JAVA_ENCODED"] } else { &[] },
        );
        source.push_str(
            r#"
@vertex fn colour_probe(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return cloud_vertex_output(vec4(uv * 2.0 - vec2(1.0), 0.5, 1.0),
        vec4(0.25, 0.5, 0.75, 0.4), 10000.0);
}
"#,
        );
        let pixels = gpu.render(
            &source,
            "colour_probe",
            &[Draw {
                fragment: "cloud_fragment",
                vertices: 0..3,
                bindings: &[wgpu::BindGroupEntry {
                    binding: 1,
                    resource: atmosphere.as_entire_binding(),
                }],
                blend: None,
                write_depth: false,
            }],
        );
        let mut expected = [0.25_f32, 0.5, 0.75, 0.4].map(|value| (value * 255.0).round() as u8);
        if !encoded {
            for (channel, value) in [0.25_f32, 0.5, 0.75].into_iter().enumerate() {
                expected[channel] = (((value + 0.055) / 1.055).powf(2.4) * 255.0).round() as u8;
            }
        }
        let actual = &pixels[(128 * 256 + 128) * 4..][..4];
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(&actual, expected)| actual.abs_diff(expected) <= 1),
            "encoded {encoded}: vertex colour and faded alpha must survive: {actual:?} != {expected:?}"
        );
    }
}

#[test]
fn native_cloud_vertices_preserve_negative_cells_subpixel_scroll_rgba_and_fade_on_gpu() {
    use bevy::math::{Mat4, Vec3};
    use gpu_snapshot::{Draw, Gpu};
    let Some(gpu) = Gpu::for_fixture("native cloud vertices and fade") else {
        return;
    };
    let source = native_probe_shader();
    let view = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut atmosphere = crate::gpu_snapshot::atmosphere();
    atmosphere[22] = -0.125 / meshing::CLOUD_WORLD_PERIOD;
    atmosphere[23] = 100.0;
    let frame = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let colour = gpu.buffer(
        &crate::gpu_snapshot::cloud_uniform(),
        wgpu::BufferUsages::UNIFORM,
    );
    let pixels = gpu.render(
        &source,
        "probe_vertex",
        &[Draw {
            fragment: "probe_fragment",
            vertices: 0..3,
            bindings: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: colour.as_entire_binding(),
                },
            ],
            blend: None,
            write_depth: false,
        }],
    );
    let expected_alpha =
        (render_scene::CLOUD_ALPHA * (1.0 - (1.4 - CLOUD_FADE_START)) * 255.0) as u8;
    for (column, expected) in [
        [126, 128, 0, 255],
        [16, 32, 64, 90],
        [255, 255, 255, expected_alpha],
        [55, 55, 55, 64],
    ]
    .into_iter()
    .enumerate()
    {
        let pixel = &pixels[(128 * 256 + column * 64 + 32) * 4..][..4];
        assert!(
            pixel
                .iter()
                .zip(expected)
                .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
            "column{column}: {pixel:?} expected{expected:?}"
        );
    }
    let interpolated = gpu.render(
        &source,
        "fade_vertex",
        &[Draw {
            fragment: "cloud_fragment",
            vertices: 0..6,
            bindings: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: colour.as_entire_binding(),
                },
            ],
            blend: None,
            write_depth: false,
        }],
    );
    let centre_alpha = interpolated[(128 * 256 + 128) * 4 + 3];
    // Native vertices at0 and200 fade1 and0; interpolation is approximately.5.
    // Recomputing distance100 per fragment would wrongly give.9 instead.
    assert!(
        centre_alpha.abs_diff(89) <= 1,
        "vertex-interpolated alpha {centre_alpha}"
    );
}

/// Builds the same probe module for interface checks and raster execution.
fn native_probe_shader() -> String {
    let mut source = shader();
    source.push_str(r#"
@vertex fn probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return cloud_vertex_output(vec4(position, 0.0, 1.0), vec4(0.0), 0.0);
}
@fragment fn probe_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let column = u32(input.position.x / view.viewport.z * 4.0);
    if column == 0u {
        let record = ViewportCloudQuad(vec2(-1, -1), FACE_DOWN, 0xffffffffu);
        let position = reconstruct_world_position(record, vec2(0.5));
        return vec4((position.xz + vec2(16.0)) / 16.0, 0.0, 1.0);
    }
    if column == 1u {
        let record = ViewportCloudQuad(vec2(0), FACE_DOWN, 0x80402010u);
        return native_cloud_vertex_colour(record, view.world_position);
    }
    if column == 2u {
        let record = ViewportCloudQuad(vec2(0), FACE_DOWN, 0xffffffffu);
        return native_cloud_vertex_colour(record, view.world_position + vec3(140.0, 0.0, 0.0));
    }
    return tint_to_linear(vec4(vec3(0.5), 0.25));
}
@vertex fn fade_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = corner_uv(index);
    let record = ViewportCloudQuad(vec2(0), FACE_DOWN, 0xffffffffu);
    let colour = native_cloud_vertex_colour(record, view.world_position + vec3(uv.x * 200.0, 0.0, 0.0));
    return cloud_vertex_output(vec4(uv * 2.0 - vec2(1.0), 0.0, 1.0), colour, uv.x * 200.0);
}
"#);
    source
}

#[test]
fn native_cloud_probe_shader_validates_without_an_adapter() {
    validate_probe(&native_probe_shader());
}

/// Validates complete probe modules before an adapter can hide a shader error.
fn validate_probe(source: &str) {
    let module = naga::front::wgsl::parse_str(source).expect("cloud probe parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("cloud probe validates");
}

#[test]
fn java_cloud_face_shading_preserves_texture_rgb_and_weather_alpha() {
    use gpu_snapshot::{Draw, Gpu};
    let Some(gpu) = Gpu::for_fixture("coloured Java cloud faces") else {
        return;
    };
    let view = gpu.buffer(
        &gpu_snapshot::view(bevy::math::Mat4::IDENTITY, bevy::math::Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut atmosphere = crate::gpu_snapshot::atmosphere();
    atmosphere[36] = 1.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let cloud = gpu.buffer(
        &[0.5, 0.75, 1.0, 0.6, 0.0, 0.0, 0.0, 0.0],
        wgpu::BufferUsages::UNIFORM,
    );
    for encoded in [false, true] {
        let source = format!(
            "{}\n{}",
            shader_source::standalone(
                include_str!("../../src/cloud.wesl"),
                if encoded { &["JAVA_ENCODED"] } else { &[] },
            ),
            COLOURED_FACE_PROBE
        );
        validate_probe(&source);
        let pixels = gpu.render(
            &source,
            "coloured_face_vertex",
            &[Draw {
                fragment: "cloud_fragment",
                vertices: 0..36,
                bindings: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: view.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: atmosphere.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: cloud.as_entire_binding(),
                    },
                ],
                blend: None,
                write_depth: false,
            }],
        );
        gpu_snapshot::save(
            if encoded {
                "coloured-java-cloud-faces-encoded"
            } else {
                "coloured-java-cloud-faces-linear"
            },
            &pixels,
        );
        for (column, shade) in [0.7, 1.0, 0.8, 0.8, 0.9, 0.9].into_iter().enumerate() {
            let rgb = [64.0 / 255.0 * 0.5, 192.0 / 255.0 * 0.75, 128.0 / 255.0];
            let expected = rgb.map(|channel| {
                let value: f32 = channel * shade;
                let value = if encoded || value <= 0.04045 {
                    if encoded { value } else { value / 12.92 }
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                };
                (value * 255.0).round() as u8
            });
            let x = (column * 256 + 128) / 6;
            let actual = &pixels[(128 * 256 + x) * 4..][..4];
            assert!(
                actual[..3]
                    .iter()
                    .zip(expected)
                    .all(|(&actual, expected)| actual.abs_diff(expected) <= 1),
                "encoded {encoded}, face {column}: {actual:?} != {expected:?}"
            );
            assert!(
                actual[3].abs_diff(153) <= 1,
                "weather alpha must survive face shading"
            );
        }
    }
}

const COLOURED_FACE_PROBE: &str = r#"
@vertex fn coloured_face_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let column = index / 6u;
    let corner = corner_uv(index % 6u);
    let face = array(FACE_DOWN, FACE_UP, FACE_NORTH, FACE_SOUTH, FACE_WEST, FACE_EAST)[column];
    let record = ViewportCloudQuad(vec2(0), face, 0xff80c040u);
    return cloud_vertex_output(
        vec4((vec2(f32(column), 0.0) + corner * vec2(1.0, 6.0)) / 6.0 * 2.0 - vec2(1.0), 0.5, 1.0),
        native_cloud_vertex_colour(record, view.world_position),
        0.0,
    );
}
"#;
