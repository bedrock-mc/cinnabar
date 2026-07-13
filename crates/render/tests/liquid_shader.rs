#[path = "support/liquid_shader_contract.rs"]
mod liquid_shader_contract;

use liquid_shader_contract::LiquidShaderContract;
use render::Face;

const SHADER: &str = include_str!("../src/liquid.wgsl");

fn shader_for_naga() -> String {
    SHADER.replacen(
        "#import bevy_render::view::View",
        "struct View { clip_from_world: mat4x4<f32>, }",
        1,
    )
}

fn assert_uv_close(actual: [f32; 2], expected: [f32; 2], context: &str) {
    assert!(
        (actual[0] - expected[0]).abs() < 1.0e-6 && (actual[1] - expected[1]).abs() < 1.0e-6,
        "{context}: expected {expected:?}, got {actual:?}",
    );
}

#[test]
fn liquid_shader_parses_validates_and_uses_shared_address_contract() {
    let shader = shader_for_naga();
    let module = naga::front::wgsl::parse_str(&shader).expect("parse liquid WGSL");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate liquid WGSL");

    assert_eq!(shader.matches("@group(0) @binding(").count(), 15);
    for binding in 0..=14 {
        assert!(shader.contains(&format!("@group(0) @binding({binding})")));
    }
    assert!(shader.contains("struct TransparentDrawRef"));
    assert!(shader.contains("liquid_record_index: u32"));
    assert!(shader.contains("metadata_index: u32"));
    assert!(shader.contains("let liquid_word = draw_ref.liquid_record_index * 4u"));
    assert!(shader.contains("let lighting_record_index = geometry_streams[liquid_word + 3u]"));
    assert!(shader.contains("geometry_streams[lighting_record_index * 2u + corner / 2u]"));
}

#[test]
fn liquid_shader_cpu_and_wgsl_decode_share_task12_winding_and_height_scale() {
    let contract = LiquidShaderContract::parse(SHADER);
    let cases = [
        (
            Face::NegativeX,
            [0, 10, 40, 0],
            [
                [3.0, 4.0, 5.0],
                [3.0, 4.0 + 10.0 / 255.0, 5.0],
                [3.0, 4.0 + 40.0 / 255.0, 6.0],
                [3.0, 4.0, 6.0],
            ],
        ),
        (
            Face::PositiveX,
            [0, 30, 20, 0],
            [
                [4.0, 4.0, 6.0],
                [4.0, 4.0 + 30.0 / 255.0, 6.0],
                [4.0, 4.0 + 20.0 / 255.0, 5.0],
                [4.0, 4.0, 5.0],
            ],
        ),
        (
            Face::NegativeY,
            [0, 0, 0, 0],
            [
                [3.0, 4.0, 5.0],
                [3.0, 4.0, 6.0],
                [4.0, 4.0, 6.0],
                [4.0, 4.0, 5.0],
            ],
        ),
        (
            Face::PositiveY,
            [10, 20, 30, 40],
            [
                [3.0, 4.0 + 10.0 / 255.0, 5.0],
                [4.0, 4.0 + 20.0 / 255.0, 5.0],
                [4.0, 4.0 + 30.0 / 255.0, 6.0],
                [3.0, 4.0 + 40.0 / 255.0, 6.0],
            ],
        ),
        (
            Face::NegativeZ,
            [0, 20, 10, 0],
            [
                [4.0, 4.0, 5.0],
                [4.0, 4.0 + 20.0 / 255.0, 5.0],
                [3.0, 4.0 + 10.0 / 255.0, 5.0],
                [3.0, 4.0, 5.0],
            ],
        ),
        (
            Face::PositiveZ,
            [0, 40, 30, 0],
            [
                [3.0, 4.0, 6.0],
                [3.0, 4.0 + 40.0 / 255.0, 6.0],
                [4.0, 4.0 + 30.0 / 255.0, 6.0],
                [4.0, 4.0, 6.0],
            ],
        ),
    ];
    for (face, (_, heights, expected)) in cases.into_iter().enumerate() {
        assert_eq!(
            std::array::from_fn(|corner| {
                contract.corner(face, corner, [3.0, 4.0, 5.0], heights[corner])
            }),
            expected,
            "Task 12 winding mismatch for face {face}",
        );
    }
}

#[test]
fn liquid_shader_cpu_and_wgsl_uv_rules_cover_still_flow_side_and_falling() {
    let contract = LiquidShaderContract::parse(SHADER);
    let face_uvs = [
        [[0.0, 1.0], [0.0, 0.498_039_22], [1.0, 0.0], [1.0, 1.0]],
        [[0.0, 1.0], [0.0, 0.498_039_22], [1.0, 0.0], [1.0, 1.0]],
        [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        [[0.0, 1.0], [0.0, 0.498_039_22], [1.0, 0.0], [1.0, 1.0]],
        [[0.0, 1.0], [0.0, 0.498_039_22], [1.0, 0.0], [1.0, 1.0]],
    ];
    for (face, expected) in face_uvs.into_iter().enumerate() {
        let heights = [0, 128, 255, 0];
        for (corner, expected) in expected.into_iter().enumerate() {
            assert_uv_close(
                contract.uv(face, corner, heights[corner], [0, 0], false, [0.0, 0.0]),
                expected,
                &format!("base UV mismatch for face {face}, corner {corner}"),
            );
        }
    }
    assert_eq!(
        contract.uv(3, 2, 128, [0, 0], false, [0.0, 0.0]),
        [1.0, 1.0]
    );
    let cardinal_flow = [
        ([1, 0], [0.0, 1.0]),
        ([0, 1], [0.0, 0.0]),
        ([-1, 0], [1.0, 0.0]),
        ([0, -1], [1.0, 1.0]),
    ];
    for (flow, expected) in cardinal_flow {
        let actual = contract.uv(3, 0, 128, flow, false, [0.0, 0.0]);
        assert!((actual[0] - expected[0]).abs() < 1.0e-6, "flow {flow:?}");
        assert!((actual[1] - expected[1]).abs() < 1.0e-6, "flow {flow:?}");
    }
    let side = contract.uv(1, 3, 128, [0, 0], false, [0.0, 0.0]);
    assert_eq!(side, [1.0, 1.0 - 128.0 / 255.0]);
    let falling = contract.uv(1, 3, 128, [0, 0], true, [4.0, 0.0]);
    assert!((falling[1].fract() - side[1].fract()).abs() > 0.1);
}

#[test]
fn shader_contract_decoder_observes_wgsl_table_and_operator_mutations() {
    let baseline = LiquidShaderContract::parse(SHADER);

    let face_table_mutation = SHADER.replacen(
        "vec2(0.0, 0.0), vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0)",
        "vec2(1.0, 0.0), vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0)",
        1,
    );
    let changed_table = LiquidShaderContract::parse(&face_table_mutation);
    assert_ne!(
        baseline.corner(0, 0, [0.0; 3], 0),
        changed_table.corner(0, 0, [0.0; 3], 0),
    );

    let angle_mutation = SHADER.replacen(
        "FLOW_ANGLE_DIRECTION * atan2(f32(flow_z), f32(flow_x))",
        "-FLOW_ANGLE_DIRECTION * atan2(f32(flow_z), f32(flow_x))",
        1,
    );
    let changed_angle = LiquidShaderContract::parse(&angle_mutation);
    assert_ne!(
        baseline.uv(3, 0, 255, [0, 1], false, [0.0, 0.0]),
        changed_angle.uv(3, 0, 255, [0, 1], false, [0.0, 0.0]),
    );

    let side_mutation = SHADER.replacen(
        "SIDE_HEIGHT_BIAS + SIDE_HEIGHT_SCALE * height",
        "SIDE_HEIGHT_BIAS - SIDE_HEIGHT_SCALE * height",
        1,
    );
    let changed_side = LiquidShaderContract::parse(&side_mutation);
    assert_ne!(
        baseline.uv(0, 1, 128, [0, 0], false, [0.0, 0.0]),
        changed_side.uv(0, 1, 128, [0, 0], false, [0.0, 0.0]),
    );

    let falling_mutation = SHADER.replacen(
        "uv.y += FALLING_SCROLL_DIRECTION * falling_phase",
        "uv.y -= FALLING_SCROLL_DIRECTION * falling_phase",
        1,
    );
    let changed_falling = LiquidShaderContract::parse(&falling_mutation);
    assert_ne!(
        baseline.uv(0, 1, 128, [0, 0], true, [4.0, 0.0]),
        changed_falling.uv(0, 1, 128, [0, 0], true, [4.0, 0.0]),
    );
}

#[test]
fn liquid_shader_preserves_straight_alpha_animation_tint_and_light() {
    assert!(SHADER.contains("fn animation_sample("));
    assert!(SHADER.contains("textureSampleGrad(block_textures_page_0"));
    assert!(SHADER.contains("textureSampleGrad(block_textures_page_1"));
    assert!(SHADER.contains("mix(current_sample, next_sample, in.frame_blend)"));
    assert!(SHADER.contains("out.water_tint = unpack_linear_rgb10(tint.water)"));
    assert!(
        SHADER.contains("return vec4(sampled.rgb * in.water_tint * in.light_factor, sampled.a)")
    );
    assert!(!SHADER.contains("sampled.rgb * sampled.a"));
    assert!(!SHADER.contains("sampled.a <"));
}

#[test]
fn liquid_shader_has_mutually_exclusive_water_and_depth_writing_entries() {
    assert!(SHADER.contains("fn vertex_depth("));
    assert!(SHADER.contains("fn fragment_depth("));
    assert!(SHADER.contains("@interpolate(flat) depth_write_route: u32"));
    assert!(SHADER.contains("LIQUID_DEPTH_WRITE_BIT"));
    assert!(SHADER.contains("let material = materials[packed_material & ~LIQUID_DEPTH_WRITE_BIT]"));
    assert!(
        SHADER.contains("let draw_ref = TransparentDrawRef(instance_index, vertex_index / 4u)")
    );
    assert!(SHADER.contains("out.depth_write_route = packed_material >> 31u"));
    assert!(SHADER.contains("if (in.depth_write_route != 0u)"));
    assert!(SHADER.contains("if (in.depth_write_route == 0u)"));
}

#[test]
fn liquid_shader_resolves_block_biome_tint_before_fragment_rasterization() {
    let vertex = SHADER
        .split("@fragment")
        .next()
        .expect("liquid shader must retain vertex stages before its fragment stages");
    let fragment = SHADER
        .split("@fragment")
        .nth(1)
        .expect("liquid shader must retain a fragment stage");

    assert!(SHADER.contains("@location(4) @interpolate(flat) water_tint: vec3<f32>"));
    assert!(vertex.contains("let block_coordinate = vec3<u32>("));
    assert!(vertex.contains("geometry & 15u"));
    assert!(vertex.contains("(geometry >> 4u) & 15u"));
    assert!(vertex.contains("(geometry >> 8u) & 15u"));
    assert!(
        vertex.contains("packed_biome_tint_index(u32(chunk_origin.value.w), block_coordinate)")
    );
    assert!(vertex.contains(
        "let tint_index = select(0u, requested_tint, requested_tint < arrayLength(&biome_tints))"
    ));
    assert!(vertex.contains("let tint = biome_tints[tint_index]"));
    assert!(vertex.contains("out.water_tint = unpack_linear_rgb10(tint.water)"));
    for forbidden in [
        "packed_biome_tint_index",
        "biome_records",
        "biome_tints[",
        "unpack_linear_rgb10",
    ] {
        assert!(
            !fragment.contains(forbidden),
            "liquid fragment stage repeated vertex-only tint work: {forbidden}",
        );
    }
    assert!(fragment.contains("sampled.rgb * in.water_tint * in.light_factor"));
}
