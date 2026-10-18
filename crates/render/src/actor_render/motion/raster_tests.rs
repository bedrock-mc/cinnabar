//! Pixel witnesses through the production actor vertex and velocity calculation.

use super::{ActorHistory, CameraHistory, tests::shader_source};
use crate::gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
use bevy::math::{Mat4, UVec4, Vec3};
use render_model::{ActorRigGeometrySpan, ActorRigVertex};
use wgpu::util::DeviceExt;

const VELOCITY_SCALE: f32 = 32.0;

/// Uses one binding group for the fixture and encodes signed velocity into visible channels.
fn velocity_shader() -> String {
    format!(
        r#"{}
@fragment fn velocity_witness(input: VertexOutput) -> @location(0) vec4<f32> {{
    return vec4(actor_motion_velocity(input) * {VELOCITY_SCALE} + vec2(0.5), 1.0, 1.0);
}}
"#,
        shader_source(&["ACTOR_MOTION"])
            .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
            .replace("@group(1) @binding(1)", "@group(0) @binding(21)")
            .replace("@group(2) @binding(0)", "@group(0) @binding(22)")
            .replace("@group(2) @binding(1)", "@group(0) @binding(23)")
    )
}

/// Draws one actor with the same buffers and interpolated poses as the velocity pass.
fn velocity_pixel(
    gpu: &Gpu,
    source: &str,
    origin: Vec3,
    previous_pose: f32,
    current_pose: f32,
) -> [u8; 4] {
    let viewport = UVec4::new(0, 0, SNAPSHOT_SIDE, SNAPSHOT_SIDE);
    let projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_2,
        1.0,
        0.1,
    );
    let previous_camera =
        CameraHistory::new(projection, Mat4::from_translation(origin), viewport, 0);
    let origin = origin + Vec3::X;
    let world_from_view = Mat4::from_translation(origin);
    let camera = CameraHistory::new(projection, world_from_view, viewport, 0);
    let mut history = ActorHistory::default();
    let frame = crate::motion_blur::actor_history::tests::frame;
    history.advance(
        &frame(
            origin - Vec3::X - Vec3::Z,
            [previous_pose, previous_pose, 1.0],
        ),
        previous_camera,
        previous_camera,
        false,
    );
    let current = frame(origin - Vec3::Z, [current_pose, current_pose, 1.0]);
    history.advance(&current, previous_camera, camera, true);

    let clip_from_world = projection * world_from_view.inverse();
    let mut view_words: Vec<f32> = [
        clip_from_world,
        clip_from_world,
        world_from_view.inverse(),
        world_from_view,
        projection,
        projection.inverse(),
    ]
    .into_iter()
    .flat_map(|matrix| matrix.to_cols_array())
    .collect();
    view_words.extend(origin.to_array());
    view_words.push(1.0);
    view_words.extend([0.0, 0.0, SNAPSHOT_SIDE as f32, SNAPSHOT_SIDE as f32]);
    let view = gpu.buffer(&view_words, wgpu::BufferUsages::UNIFORM);
    let storage = |bytes: &[u8]| {
        gpu.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("actor velocity precision fixture"),
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE,
            })
    };
    let instances = storage(bytemuck::cast_slice(&current.instances));
    let vertices = storage(bytemuck::cast_slice(
        &[[-1.0, -1.0, 0.0], [3.0, -1.0, 0.0], [-1.0, 3.0, 0.0]].map(|position| ActorRigVertex {
            position,
            normal: [0.0, 0.0, 1.0],
            ..Default::default()
        }),
    ));
    let spans = storage(bytemuck::bytes_of(&ActorRigGeometrySpan {
        first_vertex: 0,
        vertex_count: 3,
    }));
    let previous_bones = storage(bytemuck::cast_slice(&current.previous_bones));
    let current_bones = storage(bytemuck::cast_slice(&current.current_bones));
    let motion = storage(bytemuck::cast_slice(&history.instances));
    let rendered_bones = storage(bytemuck::cast_slice(&history.bones));
    let lightmap = gpu.lightmap(&[1.0; 256 * 4]);
    let bindings = [
        (0, &view),
        (1, &instances),
        (2, &vertices),
        (3, &spans),
        (4, &previous_bones),
        (5, &current_bones),
        (20, &lightmap),
        (22, &motion),
        (23, &rendered_bones),
    ]
    .map(|(binding, buffer)| wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    });
    let pixels = gpu.render(
        source,
        "actor_vertex",
        &[Draw {
            fragment: "velocity_witness",
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: false,
        }],
    );
    let offset = ((SNAPSHOT_SIDE / 2 * SNAPSHOT_SIDE + SNAPSHOT_SIDE / 2) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
}

#[test]
fn actor_shader_preserves_zero_following_velocity_and_real_sub_block_motion() {
    let limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
        ..Default::default()
    };
    let Some(gpu) = Gpu::for_fixture_with_limits(
        "actor velocity precision raster",
        wgpu::Features::empty(),
        limits,
    ) else {
        return;
    };
    let source = velocity_shader();
    for origin in [
        Vec3::ZERO,
        Vec3::splat(1_000_000.0),
        Vec3::splat(-1_000_000.0),
    ] {
        let previous_pose = 0.02;
        let current_pose = 0.04;
        let followed = velocity_pixel(&gpu, &source, origin, previous_pose, previous_pose);
        assert!(
            followed[..2]
                .iter()
                .all(|&channel| channel.abs_diff(128) <= 1),
            "camera-followed actor acquired velocity at {origin:?}: {followed:?}"
        );
        let moving = velocity_pixel(&gpu, &source, origin, previous_pose, current_pose);
        let expected =
            ((0.5 + (current_pose - previous_pose) * 0.5 * VELOCITY_SCALE) * 255.0).round() as u8;
        assert!(
            moving[0].abs_diff(expected) <= 2,
            "sub-block actor motion was lost at {origin:?}: {moving:?}"
        );
        assert!(moving[1].abs_diff(128) <= 1);
    }
}
