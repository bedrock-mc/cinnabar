use render_model::{LightmapInputs, darkness_pulse};

/// Compares independently calculated RGB fixtures without display encoding.
fn close(actual: [f32; 3], expected: [f32; 3]) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() < 0.000_002, "{actual:?} != {expected:?}");
    }
}

#[test]
fn rm01_block_sky_addition_and_sunrise() {
    let base = LightmapInputs::default();
    close(base.colour(0.25, 0.0), [0.4, 0.3136, 0.1984]);
    close(base.colour(0.25, 0.25), [0.65, 0.5636, 0.4484]);
    let night = LightmapInputs {
        sky_darken: 0.0,
        ..base
    };
    close(night.colour(0.0, 1.0), [0.0175, 0.0175, 0.05]);
    close(
        LightmapInputs {
            lightning: true,
            ..night
        }
        .colour(0.0, 1.0),
        [0.35, 0.35, 1.0],
    );
    close(
        LightmapInputs {
            sunrise: [1.0, 0.5, 0.0, 1.0],
            ..night
        }
        .colour(0.0, 1.0),
        [0.11725, 0.06475, 0.035],
    );
}

#[test]
fn rm02_gamma_endpoints_and_both_ambient_stages() {
    let base = LightmapInputs::default();
    close(base.colour(0.0, 0.0), [0.0; 3]);
    close(
        LightmapInputs {
            ambient_adjustment: true,
            ..base
        }
        .colour(0.0, 0.0),
        [0.0588; 3],
    );
    for gamma in [0.0, 0.5, 1.0] {
        let expected = 0.25 + (0.68359375 - 0.25) * gamma;
        close(
            LightmapInputs {
                brightness: gamma,
                ..base
            }
            .colour(0.0, 0.25),
            [expected; 3],
        );
    }
}

#[test]
fn rm03_night_vision_normalizes_block_light_and_darkness_subtracts_all_channels() {
    let base = LightmapInputs::default();
    close(
        LightmapInputs {
            night_vision: 1.0,
            ..base
        }
        .colour(0.25, 0.0),
        [1.0, 0.784, 0.496],
    );
    close(
        LightmapInputs {
            brightness: 0.5,
            darkness: 0.5,
            darkness_pulse: 0.1,
            ..base
        }
        .colour(0.25, 0.0),
        [0.3, 0.2136, 0.0984],
    );
    assert_eq!(darkness_pulse(0.0, 0.0, 1.0, 1.0, 0.45), 0.45);
    assert_eq!(darkness_pulse(40.0, 0.0, 1.0, 1.0, 0.45), 0.0);
    assert!((darkness_pulse(0.5, 0.5, 0.0, 1.0, 0.45) - 0.225).abs() < 1e-6);
}

#[test]
fn all_256_pairs_remain_finite_and_dimension_builder_has_ambient_color() {
    for sky_darken in [0.0, 0.5, 1.0] {
        let inputs = LightmapInputs {
            sky_darken,
            ..Default::default()
        };
        for (index, color) in inputs.build().into_iter().enumerate() {
            assert!(
                color
                    .into_iter()
                    .all(|c| c.is_finite() && (0.0..=1.0).contains(&c))
            );
            close(
                [color[0], color[1], color[2]],
                inputs.colour(inputs.ramp[index & 15], inputs.ramp[index >> 4]),
            );
        }
    }
    close(
        LightmapInputs {
            dimension_tint: Some([1.0; 3]),
            ..Default::default()
        }
        .colour(0.0, 0.0),
        [0.22, 0.28, 0.25],
    );
}

/// Enforces the actual GPU layout and removes quantization operations from light lookups.
#[test]
fn native_light_lookup_reads_the_uploaded_byte_table_without_fragment_quantization() {
    let source = crate::shader_source::standalone(include_str!("../../src/lighting.wesl"), &[]);
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let (_, uniform) = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some("LightmapUniform"))
        .unwrap();
    let naga::TypeInner::Struct { span, .. } = uniform.inner else {
        panic!("lightmap is a struct");
    };
    assert_eq!(
        span as usize,
        std::mem::size_of::<render_model::LightmapUniform>()
    );
    let (_, lookup) = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("native_light_texel"))
        .unwrap();
    assert!(
        lookup.expressions.iter().all(|(_, expression)| !matches!(
            expression,
            naga::Expression::Math {
                fun: naga::MathFunction::Floor | naga::MathFunction::Clamp,
                ..
            } | naga::Expression::Binary {
                op: naga::BinaryOperator::Divide,
                ..
            }
        )),
        "native texels must already be quantized before shading"
    );
}

/// Compares uploaded bytes with the old GPU quantizer while checking unchanged raw lookup values.
#[test]
fn uploaded_native_lightmap_matches_gpu_quantization_at_byte_boundaries() {
    use crate::gpu_snapshot::{Draw, Gpu};
    let Some(gpu) = Gpu::for_fixture("uploaded lightmap byte comparison") else {
        return;
    };
    let raw: [[f32; 4]; 256] = std::array::from_fn(|byte| {
        let boundary = byte as f32 / 255.0;
        [boundary.next_down(), boundary, boundary.next_up(), 1.0]
    });
    let lightmap = gpu.lightmap(bytemuck::cast_slice(&raw));
    let mut source = crate::shader_source::standalone(include_str!("../../src/lighting.wesl"), &[])
        .replace("@group(1) @binding(0)", "@group(0) @binding(0)");
    source.push_str(
        r#"
@vertex fn light_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return vec4(vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0), 0.0, 1.0);
}
@fragment fn light_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let old = floor(clamp(light_colour(index), vec3(0.0), vec3(1.0)) * 255.0) / 255.0;
    let difference = abs(native_light_texel(index) - old);
    return vec4(select(vec3(0.0), vec3(1.0), difference > vec3(0.0000001)), 1.0);
}
"#,
    );
    let pixels = gpu.render(
        &source,
        "light_vertex",
        &[Draw {
            fragment: "light_fragment",
            vertices: 0..3,
            bindings: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: lightmap.as_entire_binding(),
            }],
            blend: None,
            write_depth: false,
        }],
    );
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [0, 0, 0]),
        "uploaded quantization changed a native sample"
    );
}
