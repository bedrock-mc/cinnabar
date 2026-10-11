//! Execute the current native pad position hash and quarter turns on the GPU.
use crate::gpu_snapshot;

use gpu_snapshot::{Draw, Gpu};

fn shader() -> String {
    let source = include_str!("../../src/model.wesl");
    let functions = source.split_once("fn rotate_cross(").unwrap().1;
    // Up to the first preprocessor directive or entry point that follows the helpers.
    let end = ["\n#", "\n@"]
        .into_iter()
        .filter_map(|marker| functions.find(marker))
        .min()
        .unwrap();
    format!("fn rotate_cross({}\n{WITNESS}", &functions[..end])
}

#[test]
fn production_pad_hash_shader_is_valid() {
    let module = naga::front::wgsl::parse_str(&shader()).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn native_pad_rotations_execute_for_positive_negative_and_wrapping_positions() {
    let gpu = Gpu::new().expect("native GPU");
    let source = shader();
    // Current position hash, including all four rotations and signed
    // coordinates. Fixed expected cases avoid duplicating the hash in Rust.
    for (position, rotation) in [
        ([0, 64, 0], 2),
        ([1, 64, 0], 3),
        ([2, 64, 0], 1),
        ([41, 64, -106], 1),
        ([-1, 64, -1], 0),
        ([-17, 63, -112], 2),
        ([i32::MAX, i32::MIN, i32::MIN], 0),
    ] {
        let buffer = gpu.buffer(
            bytemuck::cast_slice(&[position[0], position[1], position[2], rotation]),
            wgpu::BufferUsages::UNIFORM,
        );
        let pixels = gpu.render(
            &source,
            "witness_vertex",
            &[Draw {
                fragment: "witness_fragment",
                vertices: 0..3,
                bindings: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
                blend: None,
                write_depth: false,
            }],
        );
        assert_eq!(
            &pixels[(128 * 256 + 128) * 4..][..4],
            &[0, 255, 0, 255],
            "{position:?}"
        );
    }
}

const WITNESS: &str = r#"
@group(0) @binding(0) var<uniform> position_and_expected: vec4<i32>;
struct WitnessOutput { @builtin(position) position: vec4<f32>, }
@vertex fn witness_vertex(@builtin(vertex_index) index: u32) -> WitnessOutput {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return WitnessOutput(vec4(uv * 2.0 - vec2(1.0), 0.5, 1.0));
}
@fragment fn witness_fragment() -> @location(0) vec4<f32> {
    let rotation = lily_pad_rotation(position_and_expected.xyz);
    let corners = array(vec3(0., 0.015625, 1.), vec3(0., 0.015625, 0.), vec3(1., 0.015625, 0.), vec3(1., 0.015625, 1.));
    let actual = rotate_cross(vec3(0., 0.015625, 1.), rotation);
    let expected = u32(position_and_expected.w);
    return select(vec4(1., 0., 0., 1.), vec4(0., 1., 0., 1.), rotation == expected && distance(actual, corners[expected]) < 0.000001);
}
"#;
