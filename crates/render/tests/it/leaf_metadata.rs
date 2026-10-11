//! Native leaf metadata reaches the actual GPU without changing carried art.
use crate::gpu_snapshot;
use crate::shader_source;

use gpu_snapshot::{Draw, Gpu};

fn native_rotation(position: [i32; 3]) -> u32 {
    let [x, y, z] = position.map(|value| value as u32);
    let hash = z.wrapping_mul(0x06ebfff5) ^ x.wrapping_mul(0x002fc20f) ^ y;
    let code = hash
        .wrapping_mul(0x0285b825)
        .wrapping_add(11)
        .wrapping_mul(hash)
        >> 24
        & 3;
    // Current Up/Down tessellators: code 2 is CCW, code 3 is half-turn.
    [0, 1, 3, 2][code as usize]
}

#[test]
fn gpu_native_rotation_and_ao_exponent_follow_signed_world_positions_and_pack_flags() {
    let Some(gpu) = Gpu::for_fixture(
        "gpu_native_rotation_and_ao_exponent_follow_signed_world_positions_and_pack_flags",
    ) else {
        return;
    };
    let native = assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR;
    let isotropic = assets::MATERIAL_FLAG_ISOTROPIC;
    // The pinned spruce definition authors exponent .80. Encoding is shared
    // with the compiler rather than repeating the shift/mask in the shader.
    let exponent = 80 << assets::MATERIAL_LEAF_AO_EXPONENT_SHIFT;
    let mut cases = Vec::new();
    let mut rotations_seen = [false; 4];
    for x in -17..-5 {
        let position = [x, 69, -124];
        let rotation = native_rotation(position);
        rotations_seen[rotation as usize] = true;
        for flags in [
            native | isotropic | exponent,
            isotropic,
            native | exponent,
            0,
            native | isotropic | 1,
        ] {
            let ao_face = [0.2_f32, 0.4, 0.6, 0.9][cases.len() % 4];
            let expected_rotation = if flags & isotropic != 0 && flags & 3 == 0 {
                rotation
            } else {
                flags & 3
            };
            let expected_shade = if flags & native != 0 && flags & exponent != 0 {
                ao_face.powf(0.8)
            } else {
                ao_face
            };
            cases.push((position, flags, ao_face, expected_rotation, expected_shade));
        }
    }
    assert!(rotations_seen.into_iter().all(|seen| seen));
    let mut words = Vec::new();
    for &(position, flags, ao_face, _, _) in &cases {
        words.extend(position.map(|value| f32::from_bits(value as u32)));
        words.push(f32::from_bits(flags));
        words.extend([ao_face, 0.0, 0.0, 0.0]);
    }
    let buffer = gpu.buffer(&words, wgpu::BufferUsages::STORAGE);
    let source = format!(
        "{}\n{FIXTURE}",
        shader_source::standalone(include_str!("../../src/material.wesl"), &[])
    );
    let pixels = gpu.render(
        &source,
        "metadata_vertex",
        &[Draw {
            fragment: "metadata_fragment",
            vertices: 0..3,
            bindings: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
            blend: None,
            write_depth: true,
        }],
    );
    for (index, &(_, _, _, rotation, shade)) in cases.iter().enumerate() {
        let offset = ((index / 8 * 32 + 16) * 256 + index % 8 * 32 + 16) * 4;
        assert_eq!(pixels[offset], (rotation * 85) as u8, "case {index} UV");
        assert!(
            pixels[offset + 1].abs_diff((shade * 255.0).round() as u8) <= 1,
            "case {index} AO"
        );
    }
}

const FIXTURE: &str = r#"
struct LeafMetadataCase { position_flags: vec4<u32>, shade: vec4<f32>, }
@group(0) @binding(0) var<storage, read> cases: array<LeafMetadataCase>;
@vertex fn metadata_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    return vec4(p[index], 0.5, 1.0);
}
@fragment fn metadata_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = min(u32(position.y)/32u * 8u + u32(position.x)/32u, arrayLength(&cases)-1u);
    let witness = cases[index];
    let flags = witness.position_flags.w;
    let rotation = material_uv_flags(flags, bitcast<vec3<i32>>(witness.position_flags.xyz)) & 3u;
    return vec4(f32(rotation)/3.0, material_leaf_shade(witness.shade.x, flags), 0.0, 1.0);
}
"#;
