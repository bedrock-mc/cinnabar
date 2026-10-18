use crate::{gpu_snapshot, shader_source};
use assets::{
    ResolvedFog,
    java_world::{Attribute, AttributeValue, JavaEnvironment},
};
use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
use render_model::java_world::{JavaCameraInputs, JavaVisualState};
use render_scene::AtmosphereFrame;

/// Builds native and optional fog frames with independently expected camera-relative fades.
fn fog_cases() -> Vec<(&'static str, AtmosphereFrame, [f32; 8])> {
    let native = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0).with_environment_profile(
        None,
        Some(ResolvedFog {
            start: 16.0,
            end: 20.0,
            rgb: [1.0; 3],
        }),
    );
    let mut cases = vec![(
        "native",
        native,
        [
            0.0,
            0.0,
            0.0,
            (288.0_f32.sqrt() - 16.0) / 4.0,
            1.0,
            1.0,
            0.5,
            0.5,
        ],
    )];
    let mut environment = JavaEnvironment::default();
    environment.set(Attribute::FogStart, AttributeValue([200.0; 4]));
    environment.set(Attribute::FogEnd, AttributeValue([512.0; 4]));
    environment.set(Attribute::WaterFogStart, AttributeValue([0.0; 4]));
    environment.set(Attribute::WaterFogEnd, AttributeValue([64.0; 4]));
    for (name, water, blindness, darkness, expected) in [
        (
            "java air",
            false,
            0.0,
            0.0,
            [0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 0.5, 0.5],
        ),
        (
            "java water",
            true,
            0.0,
            0.0,
            [0.0, 0.1875, 0.3125, 1.0, 1.0, 1.0, 1.0, 1.0],
        ),
        (
            "java blindness",
            false,
            1.0,
            0.0,
            [0.0, 7.0 / 15.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        ),
        (
            "java darkness",
            false,
            0.0,
            1.0,
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        ),
    ] {
        let camera = JavaCameraInputs {
            render_distance: 20.0,
            forward: [0.0, 0.0, 1.0],
            rain: 0.0,
            thunder: 0.0,
            water,
            lava: false,
            spectator: false,
            fire_resistance: false,
            brightness: 0.0,
            night_vision: 0.0,
            blindness,
            fog_darkness: darkness,
            darkness: 0.0,
            darkness_pulse: 0.0,
            sky_exposure: 1.0,
            precipitation: true,
        };
        let mut visual = JavaVisualState::default().evaluate(1, 0.0, environment, camera);
        // A white fog target exposes the fade independently of effect colour grading.
        visual.fog_colour = [1.0; 3];
        cases.push((name, native.with_java_world(visual, 0.0, 1), expected));
    }
    cases
}

#[test]
fn every_world_shader_applies_native_and_java_distance_fog() {
    let Some(gpu) = Gpu::for_fixture("world distance fog") else {
        return;
    };
    let view = gpu.buffer(
        &gpu_snapshot::view(Mat4::IDENTITY, Vec3::new(123.0, -45.0, 67.0)),
        wgpu::BufferUsages::UNIFORM,
    );
    let cases = fog_cases();
    for (name, shader) in [
        ("chunk", include_str!("../../src/chunk.wesl")),
        ("model", include_str!("../../src/model.wesl")),
        ("liquid", include_str!("../../src/liquid.wesl")),
    ] {
        let mut source = shader_source::standalone(shader, &[]);
        source.push_str(
            r#"
@vertex fn fog_probe_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return vec4(vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0), 0.0, 1.0);
}
@fragment fn fog_probe_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let offsets = array(
        vec3(0.0), vec3(3.0, 0.0, 0.0), vec3(5.0, 0.0, 0.0), vec3(12.0, 12.0, 0.0),
        vec3(18.0, 18.0, 0.0), vec3(20.0, 0.0, 0.0), vec3(0.0, 0.0, 18.0), vec3(0.0, 18.0, 0.0),
    );
    let column = u32(position.x / view.viewport.z * 8.0);
    return vec4(apply_distance_fog(vec3(0.0), view.world_position + offsets[column]), 1.0);
}
"#,
        );
        for (case, frame, expected) in &cases {
            let frame = gpu.buffer(
                &gpu_snapshot::atmosphere_words(frame),
                wgpu::BufferUsages::UNIFORM,
            );
            let pixels = gpu.render(
                &source,
                "fog_probe_vertex",
                &[Draw {
                    fragment: "fog_probe_fragment",
                    vertices: 0..3,
                    bindings: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: view.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 15,
                            resource: frame.as_entire_binding(),
                        },
                    ],
                    blend: None,
                    write_depth: false,
                }],
            );
            gpu_snapshot::save(
                &format!("world-fog-{name}-{}", case.replace(' ', "-")),
                &pixels,
            );
            for (column, expected) in expected.iter().enumerate() {
                let side = SNAPSHOT_SIDE as usize;
                let pixel = &pixels[(side / 2 * side + column * side / 8 + side / 16) * 4..][..4];
                let expected = (expected * 255.0).round() as u8;
                assert!(
                    pixel[..3]
                        .iter()
                        .all(|channel| channel.abs_diff(expected) <= 1),
                    "{name}, {case}, column {column}: {pixel:?}, expected {expected}"
                );
                assert_eq!(pixel[3], 255);
            }
        }
    }
}
