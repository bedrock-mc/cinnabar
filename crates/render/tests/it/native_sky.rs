use crate::gpu_snapshot;
use crate::shader_source;

/// The 1.26.50.26 sky mesh stores a black centre and white
/// decagon rim. Vanilla translates it to Y256 and scales it by2000.
/// The native Sky vertex shader uses that red channel to interpolate sky→fog.
#[test]
fn native_sky_fan_interpolates_center_edges_and_below_horizon_on_the_gpu() {
    use bevy::math::{Mat4, Vec3};
    use gpu_snapshot::{Draw, Gpu};
    let Some(gpu) = Gpu::for_fixture("native sky shader") else {
        return;
    };
    let mut shader = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    shader.push_str(
        r#"
@vertex fn fan_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return sky_vertex(vec4(position, 0.0, 1.0), 0.0);
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
    let frame = gpu.buffer(&[0.0; 32], wgpu::BufferUsages::UNIFORM);
    let pixels = gpu.render(
        &shader,
        "fan_probe_vertex",
        &[Draw {
            fragment: "fan_probe_fragment",
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
    let shader = include_str!("../../src/atmosphere.wesl");
    assert!(shader.contains("native_sky_fog_weight(ray)"));
    assert!(!shader.contains("smoothstep(-0.08, 0.72, ray.y)"));
    assert!(!shader.contains("colour *= 0.72"));
}

#[test]
fn native_celestial_size_basis_rain_alpha_and_star_colour_on_the_gpu() {
    use bevy::math::{Mat4, Vec3};
    use gpu_snapshot::{Draw, Gpu};
    let Some(gpu) = Gpu::for_fixture("native sky shader") else {
        return;
    };
    let mut shader = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    shader.push_str(
        r#"
@vertex fn celestial_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return sky_vertex(vec4(position, 0.0, 1.0), 0.0);
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
fn native_sky_gamma_interpolation_and_addition_survive_the_srgb_target() {
    use gpu_snapshot::{Draw, Gpu};
    let Some(gpu) = Gpu::for_fixture("native sky shader") else {
        return;
    };
    let mut shader = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    shader.push_str(
        r#"
@vertex fn colour_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let p = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return sky_vertex(vec4(p, 0.0, 1.0), 0.0);
}

@fragment fn colour_probe_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let column = u32(input.position.x / 256.0 * 4.0);
    let base = native_sky_colour(f32(column) / 3.0, input.zenith_gamma, input.horizon_gamma);
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

/// Vanilla admits orbital phase through 105/255.
/// Probe on either side, including the moon's180 offset, in the real shader.
#[test]
fn orbital_phase_visibility_and_star_brightness_survive_the_srgb_target() {
    use gpu_snapshot::{Draw, Gpu};
    let Some(gpu) = Gpu::for_fixture("native sky shader") else {
        return;
    };
    let mut shader = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    shader.push_str(
        r#"
@vertex fn phase_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let p = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return sky_vertex(vec4(p, 0.0, 1.0), 0.0);
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

/// Compares the optimized search with the original exhaustive fan over a full ray field and seams.
#[test]
fn bounded_sky_search_preserves_fan_pixels_and_limits_wedge_work() {
    use gpu_snapshot::{Draw, Gpu};
    let Some(gpu) = Gpu::for_fixture("bounded sky fan comparison") else {
        return;
    };
    let mut source = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    source = source
        .replace("fn sky_sector(point:", "fn counted_sky_sector(point:")
        .replace("sky_sector(point)", "counted_sky_sector(point)")
        .replace(
            "let middle = (lower + upper) / 2u;",
            "atomicAdd(&work[0], 1u); let middle = (lower + upper) / 2u;",
        );
    source.push_str(
        r#"
@group(0) @binding(20) var<storage, read_write> work: array<atomic<u32>>;
// Retains the previous exhaustive evaluation as an independent arithmetic witness.
fn exhaustive_fan(ray: vec3<f32>) -> f32 {
    if (ray.y <= 0.0) { return 1.0; }
    let point = ray.xz * (256.0 / 2000.0) / ray.y;
    for (var index = 0u; index < 10u; index += 1u) {
        atomicAdd(&work[1], 1u);
        let a = SKY_RING[index];
        let b = SKY_RING[(index + 1u) % 10u];
        let determinant = a.x * b.y - a.y * b.x;
        let alpha = (point.x * b.y - point.y * b.x) / determinant;
        let beta = (a.x * point.y - a.y * point.x) / determinant;
        if (alpha >= 0.0 && beta >= 0.0) { return clamp(alpha + beta, 0.0, 1.0); }
    }
    return 1.0;
}
@vertex fn search_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return vec4(vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0), 0.0, 1.0);
}
@fragment fn search_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    var point = (position.xy - vec2(128.0)) / 64.0;
    let row = u32(position.y);
    if (row < 30u) {
        let rim = SKY_RING[row % 10u];
        let jitter = select(select(-0.0000001, 0.0, row < 20u), 0.0000001, row < 10u);
        point = rim * (position.x / 128.0) + vec2(jitter, -jitter);
    }
    let ray = vec3(point.x, 256.0 / 2000.0, point.y);
    let actual = native_sky_fog_weight(ray);
    let expected = exhaustive_fan(ray);
    return vec4(select(0.0, 1.0, abs(actual - expected) > 0.000002), actual, expected, 1.0);
}
"#,
    );
    let work = gpu.words(
        &[0, 0],
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let pixels = gpu.render(
        &source,
        "search_vertex",
        &[Draw {
            fragment: "search_fragment",
            vertices: 0..3,
            bindings: &[wgpu::BindGroupEntry {
                binding: 20,
                resource: work.as_entire_binding(),
            }],
            blend: None,
            write_depth: false,
        }],
    );
    assert!(
        pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[0] == 0),
        "optimized fan changed pixels: {:?}",
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, pixel)| pixel[0] != 0)
            .take(12)
            .map(|(i, p)| ([i % 256, i / 256], p))
            .collect::<Vec<_>>()
    );
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sky wedge work counters"),
        size: 8,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&work, 0, &readback, 0, 8);
    gpu.queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let mapped = readback.slice(..).get_mapped_range().unwrap();
    let counts: &[u32] = bytemuck::cast_slice(&mapped);
    assert!(
        counts[0] > 0,
        "bounded search instrumentation did not execute"
    );
    assert!(
        counts[0] <= 3 * 256 * 256,
        "search exceeded three bisections: {counts:?}"
    );
    assert!(
        counts[0] * 3 < counts[1] * 2,
        "wedge work did not fall by a third: {counts:?}"
    );
    eprintln!(
        "sky wedge tests: bounded={}, exhaustive={}",
        counts[0], counts[1]
    );
}

/// Counts reachable transfer calls so uniform colour conversion cannot return to fragment work.
#[test]
fn sky_uniform_colour_transfers_run_only_at_vertices() {
    let source = shader_source::standalone(include_str!("../../src/atmosphere.wesl"), &[]);
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let lookup = |name: &str| {
        module
            .functions
            .iter()
            .find(|(_, function)| function.name.as_deref() == Some(name))
            .unwrap()
            .1
    };
    let gamma = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("tint_to_gamma"))
        .unwrap()
        .0;
    /// Counts transfer calls inside nested guards without depending on shader source spelling.
    fn count_calls(block: &naga::Block, target: naga::Handle<naga::Function>) -> usize {
        block
            .iter()
            .map(|statement| match statement {
                naga::Statement::Call { function, .. } => usize::from(*function == target),
                naga::Statement::If { accept, reject, .. } => {
                    count_calls(accept, target) + count_calls(reject, target)
                }
                naga::Statement::Block(inner) => count_calls(inner, target),
                _ => 0,
            })
            .sum()
    }
    assert_eq!(count_calls(&lookup("sky_vertex").body, gamma), 3);
    assert_eq!(count_calls(&lookup("native_sky_colour").body, gamma), 0);
    assert_eq!(
        count_calls(&lookup("end_sky").body, gamma),
        1,
        "only sampled texture RGB needs fragment conversion"
    );
}
