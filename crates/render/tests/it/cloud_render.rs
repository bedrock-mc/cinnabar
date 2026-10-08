#[path = "cloud_render/corners.rs"]
mod corners;
use crate::gpu_snapshot;
#[path = "cloud_render/raster.rs"]
mod raster;
use crate::shader_source;

use meshing::cloud_viewport::{CLOUD_FADE_START, shader_source as cloud_source};

fn shader() -> String {
    shader_source::standalone(&cloud_source(include_str!("../../src/cloud.wgsl")), &[])
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
    let source = include_str!("../../src/cloud.wgsl");
    let fragment = source.split("fn cloud_fragment").nth(1).unwrap();
    assert!(!fragment.contains("distance_fade"));
    assert!(!fragment.contains("cloud_colour"));
    assert!(fragment.contains("tint_to_linear(in.colour)"));
    assert!(
        source
            .contains("colour.a *= distance_fade(distance(world_position, view.world_position));")
    );
    assert!(!source.contains("cos("));
    assert!(!source.contains("RAIN_CLOUD_COLOUR"));
    assert!(!source.contains("fog_color_start.rgb"));
    assert!(!source.contains("textureSample"));
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn native_cloud_vertices_preserve_negative_cells_subpixel_scroll_rgba_and_fade_on_gpu() {
    use bevy::math::{Mat4, Vec3};
    use gpu_snapshot::{Draw, Gpu};
    let gpu = Gpu::new().expect("this fixture requires a native GPU adapter");
    let mut source = shader();
    source.push_str(r#"
@vertex fn probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(position, 0.0, 1.0), vec4(0.0));
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
    return VertexOutput(vec4(uv * 2.0 - vec2(1.0), 0.0, 1.0), colour);
}
"#);
    let view = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut atmosphere = [0.0_f32; 32];
    atmosphere[22] = -0.125 / meshing::CLOUD_WORLD_PERIOD;
    atmosphere[23] = 100.0;
    let frame = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let colour = gpu.buffer(
        &[
            1.0,
            1.0,
            1.0,
            render::CLOUD_ALPHA,
            meshing::CLOUD_CELL_BLOCKS,
            meshing::CLOUD_UNDERSIDE_Y,
            meshing::CLOUD_TOP_Y,
            meshing::CLOUD_WORLD_PERIOD,
        ],
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
    let expected_alpha = (render::CLOUD_ALPHA * (1.0 - (1.4 - CLOUD_FADE_START)) * 255.0) as u8;
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
