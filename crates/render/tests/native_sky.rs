#[path = "support/gpu_snapshot.rs"]
mod gpu_snapshot;
#[path = "support/shader_source.rs"]
mod shader_source;

/// Current 1.26.50.26 buildSkyMesh stores a black centre and white
/// decagon rim. renderSky translates it to Y256 and scales it by2000.
/// The native Sky vertex shader uses that red channel to interpolate sky→fog.
#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn native_sky_fan_interpolates_center_edges_and_below_horizon_on_the_gpu() {
    use bevy::math::{Mat4, Vec3};
    use gpu_snapshot::{Draw, Gpu};
    let gpu = Gpu::new().expect("this fixture requires a native GPU adapter");
    let mut shader = shader_source::standalone(include_str!("../src/atmosphere.wgsl"), &[]);
    shader.push_str(
        r#"
@vertex fn fan_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(position, 0.0, 1.0), 0.0);
}
@fragment fn fan_probe_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let column = u32(input.position.x / view.viewport.z * 8.0);
    let rays = array<vec3<f32>, 8>(
        vec3(0.0, 1.0, 0.0),
        vec3(1000.0, 256.0, 0.0),
        vec3(2000.0, 256.0, 0.0),
        vec3(2200.0, 256.0, 0.0),
        vec3(904.5085, 256.0, 293.8926),
        vec3(2000.0, 256.0, 1000.0),
        vec3(0.0, -1.0, 0.0),
        vec3(0.0, 0.0, 1.0),
    );
    return vec4(vec3(native_sky_fog_weight(rays[column])), 1.0);
}
"#,
    );
    let view = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let pixels = gpu.render(
        &shader,
        "fan_probe_vertex",
        &[Draw {
            fragment: "fan_probe_fragment",
            vertices: 0..3,
            bindings: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            }],
            blend: None,
            write_depth: false,
        }],
    );
    gpu_snapshot::save("native-sky-fan", &pixels);
    for (column, expected) in [0_u8, 128, 255, 255, 128, 255, 255, 255]
        .into_iter()
        .enumerate()
    {
        let pixel = &pixels[(128 * 256 + column * 32 + 16) * 4..][..4];
        assert!(
            pixel[..3]
                .iter()
                .all(|channel| channel.abs_diff(expected) <= 1),
            "native fan column {column}: {pixel:?}, expected {expected}"
        );
        assert_eq!(pixel[3], 255);
    }
}

#[test]
fn sky_uses_native_fan_interpolation_instead_of_screen_space_gradient() {
    let shader = include_str!("../src/atmosphere.wgsl");
    assert!(shader.contains("native_sky_fog_weight(ray)"));
    assert!(!shader.contains("smoothstep(-0.08, 0.72, ray.y)"));
    assert!(!shader.contains("colour *= 0.72"));
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn native_celestial_size_basis_rain_alpha_and_star_colour_on_the_gpu() {
    use bevy::math::{Mat4, Vec3};
    use gpu_snapshot::{Draw, Gpu};
    let gpu = Gpu::new().expect("this fixture requires a native GPU adapter");
    let mut shader = shader_source::standalone(include_str!("../src/atmosphere.wgsl"), &[]);
    shader.push_str(
        r#"
@vertex fn celestial_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(position, 0.0, 1.0), 0.0);
}

@fragment fn celestial_probe_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let column = u32(input.position.x / view.viewport.z * 4.0);
    if column == 0u {
        let direction = vec3(0.6, 0.8, 0.0);
        let ray = normalize(direction + vec3(0.0, 0.0, 0.250069669 * 0.5));
        return vec4(celestial_uv(ray, direction, SUN_HALF_EXTENT), 1.0);
    }
    if column == 1u {
        let direction = vec3(-0.6, 0.8, 0.0);
        let ray = normalize(direction + vec3(0.0, 0.0, 0.166660890 * 0.5));
        return vec4(celestial_uv(ray, direction, MOON_HALF_EXTENT), 1.0);
    }
    if column == 2u {
        return vec4(vec3(celestial_weather_alpha()), 1.0);
    }
    return native_star_colour(0.5);
}
"#,
    );
    let view = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut atmosphere = [0.0_f32; 32];
    atmosphere[11] = 0.25;
    atmosphere[28] = 0.5;
    let frame = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let pixels = gpu.render(
        &shader,
        "celestial_probe_vertex",
        &[Draw {
            fragment: "celestial_probe_fragment",
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
            ],
            blend: None,
            write_depth: false,
        }],
    );
    for (column, expected) in [
        [191, 128, 255, 255],
        [191, 128, 255, 255],
        [128, 128, 128, 255],
        [64, 64, 64, 128],
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
            "native celestial column {column}: {pixel:?}, expected {expected:?}"
        );
    }
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn native_sky_gamma_interpolation_and_addition_survive_the_srgb_target() {
    use gpu_snapshot::{Draw, Gpu};
    let gpu = Gpu::new().expect("this fixture requires a native GPU adapter");
    let mut shader = shader_source::standalone(include_str!("../src/atmosphere.wgsl"), &[]);
    shader.push_str(
        r#"
@vertex fn colour_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let p = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(p, 0.0, 1.0), 0.0);
}

@fragment fn colour_probe_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let column = u32(input.position.x / 256.0 * 4.0);
    let base = native_sky_colour(f32(column) / 3.0);
    if column == 3u {
        return sky_output(composite_celestial(base, vec3(0.4, 0.2, 0.8), 0.5));
    }
    return sky_output(base);
}
"#,
    );
    let linear = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let mut values = [0.0_f32; 32];
    for (offset, gamma) in [(8, [0.1, 0.3, 0.8]), (12, [0.7, 0.5, 0.2])] {
        for (channel, v) in gamma.into_iter().enumerate() {
            values[offset + channel] = linear(v);
        }
    }
    let frame = gpu.buffer(&values, wgpu::BufferUsages::UNIFORM);
    let pixels = gpu.render_srgb(
        &shader,
        "colour_probe_vertex",
        &[Draw {
            fragment: "colour_probe_fragment",
            vertices: 0..3,
            bindings: &[wgpu::BindGroupEntry {
                binding: 1,
                resource: frame.as_entire_binding(),
            }],
            blend: None,
            write_depth: false,
        }],
    );
    for (column, expected) in [
        [26_u8, 77, 204],
        [77, 94, 153],
        [128, 111, 102],
        [230, 153, 153],
    ]
    .into_iter()
    .enumerate()
    {
        let pixel = &pixels[(128 * 256 + column * 64 + 32) * 4..][..4];
        assert!(
            pixel[..3]
                .iter()
                .zip(expected)
                .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
            "column {column}: {pixel:?}, expected {expected:?}"
        );
    }
}

/// Current renderSunAndMoon admits orbital phase through 105/255.
/// Probe on either side, including the moon's180 offset, in the real shader.
#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn orbital_phase_visibility_and_star_brightness_survive_the_srgb_target() {
    use gpu_snapshot::{Draw, Gpu};
    let gpu = Gpu::new().expect("this fixture requires a native GPU adapter");
    let mut shader = shader_source::standalone(include_str!("../src/atmosphere.wgsl"), &[]);
    shader.push_str(
        r#"
@vertex fn phase_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let p = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(p, 0.0, 1.0), 0.0);
}
@fragment fn phase_probe_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let column = u32(input.position.x / 256.0 * 4.0);
    if column == 0u { return vec4(vec3(celestial_visibility(0.0)), 1.0); }
    if column == 1u { return vec4(vec3(celestial_visibility(180.0)), 1.0); }
    return tint_to_linear(native_star_colour(0.5));
}
"#,
    );
    for (degrees, sun, moon) in [
        (0.0_f32, 255_u8, 0_u8),
        (104.99, 255, 255),
        (105.01, 0, 255),
        (180.0, 0, 255),
        (254.99, 0, 255),
        (255.01, 255, 255),
    ] {
        let mut values = [0.0_f32; 32];
        values[28] = 0.5;
        values[29] = degrees / 360.0;
        let frame = gpu.buffer(&values, wgpu::BufferUsages::UNIFORM);
        let pixels = gpu.render_srgb(
            &shader,
            "phase_probe_vertex",
            &[Draw {
                fragment: "phase_probe_fragment",
                vertices: 0..3,
                bindings: &[wgpu::BindGroupEntry {
                    binding: 1,
                    resource: frame.as_entire_binding(),
                }],
                blend: None,
                write_depth: false,
            }],
        );
        for (column, expected) in [
            [sun, sun, sun, 255],
            [moon, moon, moon, 255],
            [64, 64, 64, 128],
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
                "phase {degrees}, column {column}: {pixel:?}, expected {expected:?}"
            );
        }
    }
}
#[allow(
    dead_code,
    reason = "shared shader adapter uses production material definitions"
)]
#[path = "../src/material_shader.rs"]
mod material_shader;
