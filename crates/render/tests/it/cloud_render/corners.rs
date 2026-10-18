use bevy::math::{Mat4, Vec3};

use super::gpu_snapshot::{Draw, Gpu};

#[test]
fn native_face_vertex_sequences_and_quad_diagonals_execute_on_gpu() {
    let Some(gpu) = Gpu::for_fixture("native cloud corner sequence") else {
        return;
    };
    let source = corner_probe_shader();
    let view = gpu.buffer(
        &super::gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let pixels = gpu.render(
        &source,
        "corner_probe_vertex",
        &[Draw {
            fragment: "corner_probe_fragment",
            vertices: 0..3,
            bindings: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            }],
            blend: None,
            write_depth: false,
        }],
    );
    // Vanilla emits these four vertices per
    // face. Its shared quad buffer selects [1,2,0,0,2,3].
    let corners = [
        [[1, 0], [1, 1], [0, 1], [0, 0]],
        [[0, 0], [0, 1], [1, 1], [1, 0]],
        [[0, 1], [1, 1], [1, 0], [0, 0]],
        [[0, 0], [1, 0], [1, 1], [0, 1]],
        [[0, 0], [1, 0], [1, 1], [0, 1]],
        [[0, 1], [1, 1], [1, 0], [0, 0]],
    ];
    for (row, corners) in corners.into_iter().enumerate() {
        for (column, index) in [1, 2, 0, 0, 2, 3].into_iter().enumerate() {
            let x = (column * 256 + 128) / 6;
            let y = (row * 256 + 128) / 6;
            let expected = [corners[index][0] * 255, corners[index][1] * 255, 0, 255];
            let actual = &pixels[(y * 256 + x) * 4..][..4];
            assert_eq!(actual, expected, "face{row}/vertex{column}");
        }
    }
}

/// Shares the complete corner probe with validation that needs no GPU.
fn corner_probe_shader() -> String {
    let mut source = super::shader();
    source.push_str(
        r#"
@vertex fn corner_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return cloud_vertex_output(vec4(position, 0.0, 1.0), vec4(0.0), 0.0);
}
@fragment fn corner_probe_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let face_index = u32(input.position.y / view.viewport.w * 6.0);
    let vertex_index = u32(input.position.x / view.viewport.z * 6.0);
    let face = array<u32, 6>(FACE_DOWN, FACE_UP, FACE_NORTH, FACE_SOUTH, FACE_WEST, FACE_EAST)[face_index];
    return vec4(face_corner_uv(face, vertex_index), 0.0, 1.0);
}
"#,
    );
    source
}

#[test]
fn native_cloud_corner_probe_shader_validates_without_an_adapter() {
    super::validate_probe(&corner_probe_shader());
}
