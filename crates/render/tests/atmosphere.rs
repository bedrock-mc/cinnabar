use std::sync::Arc;

use bevy::{
    math::{Mat3, Mat4, Vec3, Vec4},
    prelude::App,
    render::{
        render_resource::{
            BindingResource, DynamicUniformBuffer, ShaderType, encase::UniformBuffer,
        },
        renderer::{RenderDevice, RenderQueue, WgpuWrapper},
        view::{ColorGradingUniform, ViewUniform},
    },
};
use render::{
    AtmosphereFrame, AtmospherePlugin, DebugWorldPlugin, cloud_texture_offset, moon_phase_tile,
};

fn test_view_uniform() -> ViewUniform {
    ViewUniform {
        clip_from_world: Mat4::IDENTITY,
        unjittered_clip_from_world: Mat4::IDENTITY,
        world_from_clip: Mat4::IDENTITY,
        world_from_view: Mat4::IDENTITY,
        view_from_world: Mat4::IDENTITY,
        clip_from_view: Mat4::IDENTITY,
        view_from_clip: Mat4::IDENTITY,
        world_position: Vec3::ZERO,
        exposure: 1.0,
        viewport: Vec4::ZERO,
        main_pass_viewport: Vec4::ZERO,
        frustum: [Vec4::ZERO; 6],
        color_grading: ColorGradingUniform {
            balance: Mat3::IDENTITY,
            saturation: Vec3::ONE,
            contrast: Vec3::ONE,
            gamma: Vec3::ONE,
            gain: Vec3::ONE,
            lift: Vec3::ZERO,
            midtone_range: bevy::math::Vec2::new(0.2, 0.7),
            exposure: 0.0,
            hue: 0.0,
            post_saturation: 1.0,
        },
        mip_bias: 0.0,
        frame_count: 0,
    }
}

#[test]
fn atmosphere_plugin_is_safe_without_a_render_sub_app() {
    let mut app = App::new();
    app.add_plugins(AtmospherePlugin);
    assert!(app.world().contains_resource::<AtmosphereFrame>());
}

#[test]
fn atmosphere_and_chunk_plugins_compose_in_atmosphere_first_order() {
    let mut app = App::new();
    app.add_plugins((AtmospherePlugin, DebugWorldPlugin::new(1)));
    assert!(app.is_plugin_added::<AtmospherePlugin>());
    assert!(app.world().contains_resource::<AtmosphereFrame>());
}

#[test]
fn atmosphere_and_chunk_plugins_compose_in_chunk_first_order() {
    let mut app = App::new();
    app.add_plugins((DebugWorldPlugin::new(1), AtmospherePlugin));
    assert!(app.is_plugin_added::<AtmospherePlugin>());
    assert!(app.world().contains_resource::<AtmosphereFrame>());
}

#[test]
fn atmosphere_frame_is_a_uniform_compatible_six_vec4_abi() {
    AtmosphereFrame::assert_uniform_compat();
    let frame = AtmosphereFrame::from_bedrock_time(6_000.0, 0.25, 0.75);
    let mut encoded = UniformBuffer::new(Vec::<u8>::new());
    encoded.write(&frame).expect("encode atmosphere uniform");
    let encoded = encoded.into_inner();
    assert_eq!(AtmosphereFrame::min_size().get(), 96);
    assert_eq!(encoded.len(), 96);
    assert_eq!(encoded.as_slice(), bytemuck::bytes_of(&frame));
}

#[test]
fn moon_phase_tiles_follow_the_authoritative_four_by_two_atlas_order() {
    let expected = [
        ([0, 0], [0.0, 0.0]),
        ([32, 0], [0.25, 0.0]),
        ([64, 0], [0.5, 0.0]),
        ([96, 0], [0.75, 0.0]),
        ([0, 32], [0.0, 0.5]),
        ([32, 32], [0.25, 0.5]),
        ([64, 32], [0.5, 0.5]),
        ([96, 32], [0.75, 0.5]),
    ];
    for (phase, (pixel_origin, uv_origin)) in expected.into_iter().enumerate() {
        let tile = moon_phase_tile(phase as u8);
        assert_eq!(tile.pixel_origin, pixel_origin, "phase {phase}");
        assert_eq!(tile.uv_origin, uv_origin, "phase {phase}");
        assert_eq!(tile.uv_extent, [0.25, 0.5]);
    }
    assert_eq!(moon_phase_tile(8), moon_phase_tile(0));
    assert_eq!(moon_phase_tile(15), moon_phase_tile(7));
}

#[test]
fn cloud_motion_uses_absolute_ticks_and_wraps_euclidean_at_one_texture_period() {
    assert_eq!(cloud_texture_offset(0.0), [0.0, 0.0]);
    let one_texture_period_ticks = 256.0 / 0.03;
    let wrapped = cloud_texture_offset(one_texture_period_ticks);
    assert!(
        wrapped[0] < 1.0e-5 || wrapped[0] > 1.0 - 1.0e-5,
        "{wrapped:?}"
    );
    assert_eq!(wrapped[1], 0.0);

    let before_zero = cloud_texture_offset(-1.0);
    let before_period_end = cloud_texture_offset(one_texture_period_ticks - 1.0);
    assert!((before_zero[0] - before_period_end[0]).abs() < 1.0e-5);

    let next_day = cloud_texture_offset(24_000.0);
    assert!((next_day[0] - 0.8125).abs() < 1.0e-6, "{next_day:?}");
}

#[test]
fn sun_and_moon_visibility_overlap_only_inside_the_intentional_horizon_transition() {
    fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
        let amount = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
        amount * amount * (3.0 - 2.0 * amount)
    }
    fn visibility(vertical: f32) -> f32 {
        smoothstep(-0.04, 0.02, vertical)
    }

    for sun_vertical in [-1.0_f32, -0.5, -0.041, 0.041, 0.5, 1.0] {
        let sun = visibility(sun_vertical);
        let moon = visibility(-sun_vertical);
        assert_eq!(sun * moon, 0.0, "unexpected overlap at {sun_vertical}");
    }
    assert!(visibility(0.0) > 0.0 && visibility(-0.0) > 0.0);

    let shader = include_str!("../src/atmosphere.wgsl");
    assert!(shader.contains("fn celestial_visibility(direction_y: f32) -> f32"));
    assert_eq!(
        shader.matches("celestial_visibility(direction.y)").count(),
        2,
        "sun and moon must share one mutually exclusive horizon policy"
    );
}

#[test]
fn dynamic_view_binding_window_keeps_a_nonzero_second_view_offset_in_bounds() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let mut uniforms = DynamicUniformBuffer::<ViewUniform>::default();
    uniforms.push(&test_view_uniform());
    let second_view_offset = u64::from(uniforms.push(&test_view_uniform()));
    uniforms.write_buffer(&device, &queue);

    let BindingResource::Buffer(binding) = uniforms.binding().expect("view binding") else {
        panic!("dynamic uniforms must expose a buffer binding");
    };
    let bound_size = binding
        .size
        .expect("dynamic binding has an exact window")
        .get();
    assert_eq!(bound_size, ViewUniform::min_size().get());
    assert!(second_view_offset + bound_size <= binding.buffer.size());
    assert!(second_view_offset + binding.buffer.size() > binding.buffer.size());

    let source = include_str!("../src/atmosphere_render.rs");
    assert!(source.contains("view_uniforms.uniforms.binding()"));
    assert!(!source.contains("view_buffer.as_entire_binding()"));
}

#[test]
fn texture_backed_sky_shader_parses_validates_and_uses_reversed_z_far_depth() {
    let shader = include_str!("../src/atmosphere.wgsl").replacen(
        "#import bevy_render::view::View",
        r#"
struct View {
    clip_from_world: mat4x4<f32>,
    unjittered_clip_from_world: mat4x4<f32>,
    view_from_world: mat4x4<f32>,
    world_from_view: mat4x4<f32>,
    clip_from_view: mat4x4<f32>,
    view_from_clip: mat4x4<f32>,
    world_position: vec3<f32>,
    exposure: f32,
    viewport: vec4<f32>,
}
"#,
        1,
    );
    let module = naga::front::wgsl::parse_str(&shader).expect("parse atmosphere WGSL");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .expect("validate atmosphere WGSL");
    assert!(shader.contains("vec4(clip_position, 0.0, 1.0)"));
    assert!(shader.contains("@binding(2) var sun_texture: texture_2d<f32>;"));
    assert!(shader.contains("@binding(3) var moon_phases_texture: texture_2d<f32>;"));
    assert!(shader.contains("@binding(4) var clouds_texture: texture_2d<f32>;"));
    assert!(shader.contains("@binding(5) var atmosphere_sampler: sampler;"));
    assert!(shader.contains("textureSampleLevel(sun_texture"));
    assert!(shader.contains("textureSampleLevel(moon_phases_texture"));
    assert!(shader.contains("textureSampleLevel(clouds_texture"));
    assert!(shader.contains("let phase_column = phase % 4u;"));
    assert!(shader.contains("let phase_row = phase / 4u;"));
    assert!(shader.contains("const CLOUD_ALTITUDE: f32 = 128.0;"));
    assert!(shader.contains("fract(world_position.xz / CLOUD_TEXTURE_WORLD_PERIOD"));
    assert!(shader.contains("mix(colour, cloud_colour, cloud_alpha)"));
}

#[test]
fn atmosphere_pipeline_specializes_msaa_and_keeps_reversed_z_without_depth_writes() {
    let source = include_str!("../src/atmosphere_render.rs");
    assert!(source.contains("key.msaa.samples()"));
    assert!(source.contains("depth_write_enabled: false"));
    assert!(source.contains("depth_compare: CompareFunction::GreaterEqual"));
    assert!(source.contains("CORE_3D_DEPTH_FORMAT"));
    assert!(source.contains("BufferBindingType::Uniform"));
    assert!(source.contains("BindingType::Texture"));
    assert!(source.contains("BindingType::Sampler"));
    assert_eq!(
        source.matches("visibility: ShaderStages::FRAGMENT").count(),
        6,
        "Metal requires every fragment-read atmosphere binding to declare fragment visibility"
    );
    assert!(!source.contains("BufferBindingType::Storage"));
}

#[test]
fn frame_updates_keep_asset_uploads_and_bind_groups_identity_stable() {
    let atmosphere = include_str!("../src/atmosphere_render.rs");
    let chunks = include_str!("../src/plugin.rs");
    assert_eq!(
        atmosphere.matches("create_buffer_with_data(").count(),
        1,
        "the atmosphere uniform buffer is allocated once at render startup"
    );
    assert!(
        atmosphere
            .contains("render_queue.write_buffer(&gpu.buffer, 0, bytemuck::bytes_of(&*frame))")
    );
    assert!(chunks.contains("atmosphere: atmosphere.buffer.id()"));
    assert!(chunks.contains("resource: atmosphere.buffer.as_entire_binding()"));
    assert_eq!(
        atmosphere.matches("create_texture_with_data(").count(),
        1,
        "one helper owns all three one-time atmosphere uploads"
    );
    assert!(atmosphere.contains("prepared.identity == requested.identity()"));
    assert!(atmosphere.contains("gpu.bound_asset_identity == Some(prepared.identity)"));
}

#[test]
fn every_world_shader_uses_the_shared_distance_fog_uniform() {
    for (name, shader) in [
        ("chunk", include_str!("../src/chunk.wgsl")),
        ("model", include_str!("../src/model.wgsl")),
        ("liquid", include_str!("../src/liquid.wgsl")),
    ] {
        assert!(
            shader.contains("@group(0) @binding(15) var<uniform> atmosphere: AtmosphereUniform;"),
            "{name} is missing the shared atmosphere uniform"
        );
        assert!(
            shader.contains("fn apply_distance_fog("),
            "{name} is missing bounded distance fog"
        );
        assert!(
            shader.contains("distance(world_position, view.world_position)"),
            "{name} fog must use camera-relative world distance, not depth"
        );
    }
}

#[test]
fn transparent_world_shaders_preserve_alpha_for_single_fog_composition() {
    for (name, shader) in [
        ("model", include_str!("../src/model.wgsl")),
        ("liquid", include_str!("../src/liquid.wgsl")),
    ] {
        assert!(
            !shader.contains("mix(colour.a, 1.0, fog)"),
            "{name} must not double-count fog by making transparent alpha opaque"
        );
    }

    let source = 0.8_f32;
    let background = 0.2_f32;
    let fog_colour = 0.5_f32;
    let alpha = 0.35_f32;
    let fog = 0.7_f32;
    let fogged_source = source + (fog_colour - source) * fog;
    let fogged_background = background + (fog_colour - background) * fog;
    let composed_after_fog = alpha * fogged_source + (1.0 - alpha) * fogged_background;
    let composed_before_fog = alpha * source + (1.0 - alpha) * background;
    let fogged_composite = composed_before_fog + (fog_colour - composed_before_fog) * fog;
    assert!((composed_after_fog - fogged_composite).abs() < 1.0e-6);
}
