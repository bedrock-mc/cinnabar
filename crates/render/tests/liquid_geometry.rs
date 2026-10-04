//! Execute the production liquid geometry/UV functions on the native GPU.
#[path = "support/gpu_snapshot.rs"]
mod gpu_snapshot;
#[path = "../src/material_shader.rs"]
#[allow(dead_code, reason = "shared production shader substitutions")]
mod material_shader;
#[path = "support/shader_source.rs"]
mod shader_source;

use gpu_snapshot::{Draw, Gpu, RasterState};
use meshing::liquid::{LIQUID_FACE_INSET, LIQUID_TOP_INSET_BIT};
use meshing::{Face, PackedLiquidQuad};

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn native_liquid_planes_and_replicated_flow_uvs_execute_on_gpu() {
    let gpu = Gpu::new().expect("native GPU");
    let source = format!(
        "{}\n{WITNESS}",
        shader_source::standalone(include_str!("../src/liquid.wgsl"), &[])
    );
    for face in Face::ALL {
        for top_emitted in [false, true] {
            let heights = match face {
                Face::PositiveY => [255; 4],
                Face::NegativeY => [0; 4],
                _ => [0, 255, 255, 0],
            };
            let quad =
                PackedLiquidQuad::try_pack([3, 4, 5], face, heights, 7, 0, [0, 0], false).unwrap();
            for (corner, &height) in heights.iter().enumerate() {
                let mut packed = quad.words();
                if top_emitted {
                    packed[2] |= LIQUID_TOP_INSET_BIT;
                }
                let xz = match face {
                    Face::NegativeX => [[0., 0.], [0., 0.], [0., 1.], [0., 1.]],
                    Face::PositiveX => [[1., 1.], [1., 1.], [1., 0.], [1., 0.]],
                    Face::NegativeY => [[0., 0.], [0., 1.], [1., 1.], [1., 0.]],
                    Face::PositiveY => [[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
                    Face::NegativeZ => [[1., 0.], [1., 0.], [0., 0.], [0., 0.]],
                    Face::PositiveZ => [[0., 1.], [0., 1.], [1., 1.], [1., 1.]],
                }[corner];
                let mut expected = [3. + xz[0], 4. + f32::from(height) / 255., 5. + xz[1], 0.];
                match face {
                    Face::NegativeX => expected[0] += LIQUID_FACE_INSET,
                    Face::PositiveX => expected[0] -= LIQUID_FACE_INSET,
                    Face::NegativeZ => expected[2] += LIQUID_FACE_INSET,
                    Face::PositiveZ => expected[2] -= LIQUID_FACE_INSET,
                    _ => {}
                }
                if top_emitted
                    && face != Face::NegativeY
                    && (face == Face::PositiveY || corner == 1 || corner == 2)
                {
                    expected[1] -= LIQUID_FACE_INSET;
                }
                let record = [
                    [
                        f32::from_bits(packed[0]),
                        f32::from_bits(packed[1]),
                        f32::from_bits(packed[2]),
                        f32::from_bits(corner as u32),
                    ],
                    expected,
                    [
                        face as u32 as f32,
                        corner as f32,
                        f32::from(height) / 255.,
                        0.5,
                    ],
                    [0., 0., 0., 0.],
                ];
                assert_case(
                    &gpu,
                    &source,
                    record,
                    "geometry",
                    &format!("{face:?} corner{corner} top{top_emitted}"),
                );
            }
        }
    }
    for (flow, expected) in [
        ([1., 0.], [0.75, 0.25]),
        ([0., 1.], [0.25, 0.25]),
        ([-1., 0.], [0.25, 0.75]),
        ([0., -1.], [0.75, 0.75]),
    ] {
        assert_case(
            &gpu,
            &source,
            [
                [0.; 4],
                [0.; 4],
                [3., 0., 1., 0.5],
                [flow[0], flow[1], expected[0], expected[1]],
            ],
            "uv",
            &format!("flow {flow:?}"),
        );
    }
    for scale in [1., 0.5, 0.25] {
        let height = 1. - LIQUID_FACE_INSET;
        assert_case(
            &gpu,
            &source,
            [
                [0.; 4],
                [0.; 4],
                [1., 2., height, scale],
                [0., 0., scale, (1. - height) * scale],
            ],
            "uv",
            &format!("side replicate scale{scale}"),
        );
    }
}

fn assert_case(gpu: &Gpu, source: &str, record: [[f32; 4]; 4], fragment: &str, label: &str) {
    let buffer = gpu.buffer(bytemuck::cast_slice(&record), wgpu::BufferUsages::STORAGE);
    let bindings = [wgpu::BindGroupEntry {
        binding: 24,
        resource: buffer.as_entire_binding(),
    }];
    let pixels = gpu.render_with_state(
        source,
        "witness",
        &[Draw {
            fragment,
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: false,
        }],
        RasterState::default(),
    );
    assert_eq!(
        &pixels[(128 * 256 + 128) * 4..][..4],
        &[0, 255, 0, 255],
        "{label}"
    );
}

const WITNESS: &str = r#"
@group(0) @binding(24) var<storage, read> witness_case: array<vec4<f32>>;
struct WitnessOutput { @builtin(position) position: vec4<f32>, }
@vertex fn witness(@builtin(vertex_index) index: u32) -> WitnessOutput {
    var out: WitnessOutput;
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    out.position = vec4(uv * 2.0 - vec2(1.0), 0.5, 1.0);
    return out;
}
@fragment fn geometry() -> @location(0) vec4<f32> {
    let data = bitcast<vec4<u32>>(witness_case[0]);
    let actual = liquid_corner(data.x, data.y, data.w, data.z);
    return select(vec4(1., 0., 0., 1.), vec4(0., 1., 0., 1.), distance(actual, witness_case[1].xyz) < 0.000002);
}
@fragment fn uv() -> @location(0) vec4<f32> {
    let data = witness_case[2];
    let expected = witness_case[3];
    let actual = liquid_uv(u32(data.x), u32(data.y), data.z, i32(expected.x), i32(expected.y), data.w);
    return select(vec4(1., 0., 0., 1.), vec4(0., 1., 0., 1.), distance(actual, expected.zw) < 0.000002);
}
"#;
