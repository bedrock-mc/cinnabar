use std::sync::Arc;

#[path = "support/shader_source.rs"]
mod shader_source;

use assets::ResolvedFog;

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
    AtmosphereFrame, AtmospherePlugin, AtmosphereViewInputs, CLOUD_ALPHA, ChunkRenderPlugin,
    PROVISIONAL_BOSS_DARKEN_SKY_STRENGTH, PROVISIONAL_BOSS_WORLD_FOG_END_BLOCKS,
    PROVISIONAL_BOSS_WORLD_FOG_START_BLOCKS, cloud_colour, cloud_distance_fade, cloud_face_shade,
    cloud_texture_offset, cloud_weather_colour, moon_phase_tile,
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
    app.add_plugins((AtmospherePlugin, ChunkRenderPlugin::new(1)));
    assert!(app.is_plugin_added::<AtmospherePlugin>());
    assert!(app.world().contains_resource::<AtmosphereFrame>());
}

#[test]
fn atmosphere_and_chunk_plugins_compose_in_chunk_first_order() {
    let mut app = App::new();
    app.add_plugins((ChunkRenderPlugin::new(1), AtmospherePlugin));
    assert!(app.is_plugin_added::<AtmospherePlugin>());
    assert!(app.world().contains_resource::<AtmosphereFrame>());
}

#[test]
fn atmosphere_frame_is_a_uniform_compatible_nine_vec4_abi() {
    AtmosphereFrame::assert_uniform_compat();
    let frame = AtmosphereFrame::from_bedrock_time(6_000.0, 0.25, 0.75);
    let mut encoded = UniformBuffer::new(Vec::<u8>::new());
    encoded.write(&frame).expect("encode atmosphere uniform");
    let encoded = encoded.into_inner();
    let byte_length = std::mem::size_of::<AtmosphereFrame>();
    assert_eq!(AtmosphereFrame::min_size().get(), byte_length as u64);
    assert_eq!(encoded.len(), byte_length);
    assert_eq!(encoded.as_slice(), bytemuck::bytes_of(&frame));
}

#[test]
fn exact_environment_values_replace_only_sky_and_fog_fields() {
    let baseline = AtmosphereFrame::from_bedrock_time(18_000.0, 0.25, 0.5);
    let applied = baseline.with_environment_profile(
        Some(0x00_0000),
        Some(ResolvedFog {
            start: 235.52,
            end: 256.0,
            rgb: [11.0 / 255.0, 8.0 / 255.0, 12.0 / 255.0],
        }),
    );

    assert_eq!(applied.sky_zenith(), [0.0; 3]);
    assert_eq!(applied.sky_horizon(), [0.0; 3]);
    assert_eq!(applied.fog_start(), 235.52);
    assert_eq!(applied.fog_end(), 256.0);
    assert_eq!(applied.fog_color(), rgb8_to_linear(0x0B_08_0C));
    assert_eq!(applied.sun_direction(), baseline.sun_direction());
    assert_eq!(applied.moon_phase(), baseline.moon_phase());
    assert_eq!(applied.day_fraction(), baseline.day_fraction());
    assert_eq!(applied.rain_level(), baseline.rain_level());
    assert_eq!(applied.thunder_level(), baseline.thunder_level());
    assert_eq!(
        applied.cloud_texture_offset(),
        baseline.cloud_texture_offset()
    );
}

#[test]
fn boss_environment_without_requests_is_an_exact_identity() {
    let baseline = AtmosphereFrame::from_bedrock_time(18_000.0, 0.3, 0.2);
    let applied = baseline.with_boss_environment(false, false);
    assert_eq!(applied, baseline);
}

#[test]
fn boss_darkening_mixes_sky_toward_the_provisional_targets() {
    let baseline = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
    let darkened = baseline.with_boss_environment(true, false);

    let strength = PROVISIONAL_BOSS_DARKEN_SKY_STRENGTH;
    assert!(darkened.sky_zenith()[1] < baseline.sky_zenith()[1]);
    assert!(darkened.sky_horizon()[1] < baseline.sky_horizon()[1]);
    for channel in 0..3 {
        let expected_zenith = baseline.sky_zenith()[channel]
            + ([0.12, 0.14, 0.16][channel] - baseline.sky_zenith()[channel]) * strength;
        assert!((darkened.sky_zenith()[channel] - expected_zenith).abs() < 1e-6);
    }
    // Celestial state and weather channels stay untouched.
    assert_eq!(darkened.sun_direction(), baseline.sun_direction());
    assert_eq!(darkened.moon_phase(), baseline.moon_phase());
    assert_eq!(darkened.day_fraction(), baseline.day_fraction());
    assert_eq!(
        darkened.cloud_texture_offset(),
        baseline.cloud_texture_offset()
    );
    assert_eq!(darkened.rain_level(), baseline.rain_level());
    assert_eq!(darkened.thunder_level(), baseline.thunder_level());
    // Without a fog request both fog distances and tint are unchanged.
    assert_eq!(darkened.fog_start(), baseline.fog_start());
    assert_eq!(darkened.fog_end(), baseline.fog_end());
    assert_eq!(darkened.fog_color(), baseline.fog_color());
}

#[test]
fn boss_world_fog_pulls_distances_inward_and_derives_the_tint() {
    let baseline = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
    let fogged = baseline.with_boss_environment(false, true);

    assert_eq!(fogged.fog_start(), PROVISIONAL_BOSS_WORLD_FOG_START_BLOCKS);
    assert_eq!(fogged.fog_end(), PROVISIONAL_BOSS_WORLD_FOG_END_BLOCKS);
    assert!(fogged.fog_end() >= fogged.fog_start());
    // A fog-only request leaves the sky and celestial channels unchanged.
    assert_eq!(fogged.sky_zenith(), baseline.sky_zenith());
    assert_eq!(fogged.sky_horizon(), baseline.sky_horizon());
    assert_eq!(fogged.sun_direction(), baseline.sun_direction());
    assert_eq!(
        fogged.cloud_texture_offset(),
        baseline.cloud_texture_offset()
    );
    let zenith = baseline.sky_zenith();
    let horizon = baseline.sky_horizon();
    let expected: [f32; 3] = std::array::from_fn(|channel| {
        horizon[channel] + (zenith[channel] - horizon[channel]) * 0.18
    });
    for (channel, expected_value) in expected.iter().enumerate() {
        assert!((fogged.fog_color()[channel] - expected_value).abs() < 1e-6);
    }
}

#[test]
fn combined_boss_requests_stay_finite_and_ordered() {
    let frame = AtmosphereFrame::from_bedrock_time(12_345.0, 0.4, 0.4)
        .with_camera_medium(meshing::CameraMedium::Air)
        .with_boss_environment(true, true);
    assert!(frame.sky_zenith().iter().all(|value| value.is_finite()));
    assert!(frame.sky_horizon().iter().all(|value| value.is_finite()));
    assert!(frame.fog_color().iter().all(|value| value.is_finite()));
    assert!(frame.fog_end() >= frame.fog_start());
}

#[test]
fn boss_requests_override_a_client_profile_in_air() {
    let profiled = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0).with_environment_profile(
        Some(0x12_34_56),
        Some(ResolvedFog {
            start: 235.52,
            end: 256.0,
            rgb: [11.0 / 255.0, 8.0 / 255.0, 12.0 / 255.0],
        }),
    );
    let bossed = profiled.with_boss_environment(false, true);

    // Boss fog wins over the profile fog in air.
    assert_eq!(bossed.fog_start(), PROVISIONAL_BOSS_WORLD_FOG_START_BLOCKS);
    assert_eq!(bossed.fog_end(), PROVISIONAL_BOSS_WORLD_FOG_END_BLOCKS);
    // A fog-only request leaves the profiled sky channels exactly as set.
    assert_eq!(bossed.sky_zenith(), profiled.sky_zenith());
}

fn rgb8_to_linear(rgb: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| {
        let value = ((rgb >> shift) & 0xff) as f32 / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
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
    let one_texture_period_ticks = 4096.0 / 0.02;
    let wrapped = cloud_texture_offset(one_texture_period_ticks);
    assert!(
        wrapped[0] < 1.0e-5 || wrapped[0] > 1.0 - 1.0e-5,
        "{wrapped:?}"
    );
    assert_eq!(wrapped[1], 0.0);

    let before_zero = cloud_texture_offset(-1.0);
    let before_period_end = cloud_texture_offset(one_texture_period_ticks - 1.0);
    assert!((before_zero[0] - before_period_end[0]).abs() < 1.0e-5);

    // 24,000 ticks drift 480 blocks toward -X.
    let next_day = cloud_texture_offset(24_000.0);
    assert!(
        (next_day[0] - (1.0 - 480.0 / 4096.0)).abs() < 1.0e-6,
        "{next_day:?}"
    );
}

#[test]
fn clouds_drift_west_two_hundredths_of_a_block_per_tick() {
    fn world_x_for_feature(texture_u: f64, absolute_ticks: f64) -> f64 {
        let offset = f64::from(cloud_texture_offset(absolute_ticks)[0]);
        (texture_u + offset) * 4096.0
    }

    let start = world_x_for_feature(0.25, 0.0);
    let later = world_x_for_feature(0.25, 1500.0);
    let moved = (later - start).rem_euclid(4096.0) - 4096.0;
    assert!((moved + 30.0).abs() < 1.0e-3, "{start} -> {later}");

    let shader = include_str!("../src/cloud.wgsl");
    assert!(shader.contains("atmosphere.fog_end_time.z * native_cloud.geometry.w"));
}

#[test]
fn cloud_weather_colours_use_exact_native_values_and_contributions() {
    let clear = cloud_weather_colour(0.0, 0.0);
    let rain = cloud_weather_colour(1.0, 0.0);
    let thunder = cloud_weather_colour(0.0, 1.0);
    // Current getCloudColor, legacy (non-custom) branch.
    let rain_native = 0.6_f32;
    let thunder_native = 0.2_f32;

    assert_eq!(clear, [1.0; 3]);
    for channel in rain {
        assert!((channel - (1.0 + (rain_native - 1.0) * 0.95)).abs() < 1.0e-6);
    }
    for channel in thunder {
        assert!((channel - (1.0 + (thunder_native - 1.0) * 0.95)).abs() < 1.0e-6);
    }

    assert_eq!(cloud_weather_colour(f32::NAN, f32::INFINITY), clear);
}

#[test]
fn cloud_faces_carry_the_vanilla_baked_shade() {
    assert_eq!(cloud_face_shade([0.0, 1.0, 0.0]), 1.0);
    assert_eq!(cloud_face_shade([0.0, -1.0, 0.0]), 0.75);
    assert!((cloud_face_shade([1.0, 0.0, 0.0]) - 0.925).abs() < 1.0e-6);
    assert!((cloud_face_shade([-1.0, 0.0, 0.0]) - 0.925).abs() < 1.0e-6);
    assert_eq!(cloud_face_shade([0.0, 0.0, 1.0]), 1.0);
    assert_eq!(cloud_face_shade([f32::NAN; 3]), 1.0);
}

#[test]
fn cloud_colour_follows_day_brightness_weather_and_fixed_alpha() {
    assert_eq!(cloud_colour(0.0, 0.0, 0.0, [0.0; 4]), [1.0, 1.0, 1.0, 0.7]);
    let night = cloud_colour(0.5, 0.0, 0.0, [0.0; 4]);
    for (channel, expected) in night.into_iter().zip([0.1, 0.1, 0.15, 0.7]) {
        assert!((channel - expected).abs() < 1.0e-6, "{night:?}");
    }
    let rain = cloud_colour(0.0, 1.0, 0.0, [0.0; 4]);
    let tint = 1.0 + (0.6_f32 - 1.0) * 0.95;
    assert!((rain[0] - tint).abs() < 1.0e-6 && (rain[2] - tint).abs() < 1.0e-6);
    let dawn = cloud_colour(0.0, 0.0, 0.0, [1.0, 0.0, 0.0, 1.0]);
    assert!((dawn[1] - 0.65).abs() < 1.0e-6 && (dawn[0] - 1.0).abs() < 1.0e-6);
    assert_eq!(CLOUD_ALPHA, 0.7);
}

#[test]
fn cloud_thunder_desaturates_the_day_shaded_colour_not_the_white_prototype() {
    let night = cloud_colour(0.5, 0.0, 1.0, [0.0; 4]);
    let grey = (0.1 * 0.3 + 0.1 * 0.59 + 0.15 * 0.11) * 0.2;
    for (actual, clear) in night[..3].iter().zip([0.1, 0.1, 0.15]) {
        let expected = clear * 0.05 + grey * 0.95;
        assert!((actual - expected).abs() < 1e-6, "{night:?}");
    }
}

#[test]
fn native_sky_subtraction_uses_camera_glare_not_rain_level() {
    let clear = AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0);
    let rainy = AtmosphereFrame::from_bedrock_time(6000.0, 1.0, 0.0);
    assert_eq!(clear.sky_zenith(), rainy.sky_zenith());
    let sunward = clear.with_camera_environment(render::AtmosphereViewInputs {
        forward: [0.0, 1.0, 0.0],
        fog_weather_level: 0.0,
        ..Default::default()
    });
    let gamma = |v: f32| {
        if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    for (clear, sunward) in clear.sky_zenith().into_iter().zip(sunward.sky_zenith()) {
        assert!((gamma(clear) - 0.2 - gamma(sunward)).abs() < 1e-5);
    }
    assert_eq!(
        clear.cloud_colour_for_view(render::AtmosphereViewInputs::default()),
        [1.0, 1.0, 1.0, CLOUD_ALPHA]
    );
    assert_eq!(
        clear.cloud_colour_for_view(render::AtmosphereViewInputs {
            forward: [0.0, 1.0, 0.0],
            fog_weather_level: 0.0,
            ..Default::default()
        }),
        [0.8, 0.8, 0.8, CLOUD_ALPHA]
    );
}

fn assert_sky_gamma(frame: AtmosphereFrame, expected: [f32; 3]) {
    for (actual, expected) in frame.sky_zenith().into_iter().zip(expected) {
        let actual = if actual <= 0.003_130_8 {
            actual * 12.92
        } else {
            1.055 * actual.powf(1.0 / 2.4) - 0.055
        };
        assert!((actual - expected).abs() < 1.0e-6, "{actual} != {expected}");
    }
}

#[test]
fn native_rain_sky_requires_current_rain_above_threshold_and_precipitation_fog() {
    let frame = AtmosphereFrame::from_bedrock_time(6000.0, 1.0, 0.0);
    let wet_view = AtmosphereViewInputs {
        current_rain_level: 1.0,
        fog_weather_level: 0.25,
        ..Default::default()
    };
    for current_rain_level in [0.0, 0.2, f32::NAN, f32::INFINITY] {
        assert_eq!(
            frame
                .with_camera_environment(AtmosphereViewInputs {
                    current_rain_level,
                    ..wet_view
                })
                .sky_zenith(),
            frame.sky_zenith()
        );
    }
    for fog_weather_level in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            frame
                .with_camera_environment(AtmosphereViewInputs {
                    fog_weather_level,
                    ..wet_view
                })
                .sky_zenith(),
            frame.sky_zenith()
        );
    }
    assert_sky_gamma(
        frame.with_camera_environment(AtmosphereViewInputs {
            current_rain_level: 0.200_001,
            ..wet_view
        }),
        [0.5; 3],
    );
    // Native admission reads Weather+0x38, independently of interpolated frame rain.
    assert_sky_gamma(
        AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0).with_camera_environment(wet_view),
        [0.5; 3],
    );
}

#[test]
fn native_rain_sky_uses_fourfold_fog_weight_and_day_weighted_gray_after_profile() {
    let view = AtmosphereViewInputs {
        current_rain_level: 1.0,
        fog_weather_level: 0.125,
        ..Default::default()
    };
    let day = AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0)
        .with_environment_profile(Some(0x33_66_cc), None);
    assert_sky_gamma(day.with_camera_environment(view), [0.35, 0.45, 0.65]);
    for fog_weather_level in [0.25, 1.0] {
        assert_sky_gamma(
            day.with_camera_environment(AtmosphereViewInputs {
                fog_weather_level,
                ..view
            }),
            [0.5; 3],
        );
    }
    let night = AtmosphereFrame::from_bedrock_time(18000.0, 0.0, 0.0)
        .with_environment_profile(Some(0x33_66_cc), None);
    assert_sky_gamma(night.with_camera_environment(view), [0.0; 3]);
}

#[test]
fn native_rain_sky_preserves_thunder_after_rain_then_camera_glare() {
    let view = AtmosphereViewInputs {
        current_rain_level: 1.0,
        fog_weather_level: 0.125,
        ..Default::default()
    };
    let day = AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.4)
        .with_environment_profile(Some(0x33_66_cc), None);
    let rain_mixed = [0.35, 0.45, 0.65];
    let gray = (rain_mixed[0] * 0.3 + rain_mixed[1] * 0.59 + rain_mixed[2] * 0.11) * 0.2;
    assert_sky_gamma(
        day.with_camera_environment(view),
        rain_mixed.map(|channel| channel * 0.7 + gray * 0.3),
    );
    assert_sky_gamma(
        AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0).with_camera_environment(
            AtmosphereViewInputs {
                forward: [0.0, 1.0, 0.0],
                fog_weather_level: 0.25,
                ..view
            },
        ),
        [0.35; 3],
    );
}

#[test]
fn native_rain_sky_keeps_clear_views_and_other_dimensions_unchanged() {
    let frame = AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0);
    assert_eq!(
        frame.with_camera_environment(AtmosphereViewInputs::default()),
        frame
    );
    let wet_view = AtmosphereViewInputs {
        current_rain_level: 1.0,
        fog_weather_level: 1.0,
        ..Default::default()
    };
    for kind in [render::SkyKind::Nether, render::SkyKind::End] {
        let other = frame.with_sky_kind(kind);
        assert_eq!(other.with_camera_environment(wet_view), other);
    }
}

#[test]
fn cloud_alpha_fades_from_nine_tenths_to_nineteen_tenths_of_the_distance() {
    assert_eq!(cloud_distance_fade(0.0, 768.0), 1.0);
    assert!((cloud_distance_fade(0.9 * 768.0, 768.0) - 1.0).abs() < 1.0e-6);
    assert!((cloud_distance_fade(1.4 * 768.0, 768.0) - 0.5).abs() < 1.0e-5);
    assert_eq!(cloud_distance_fade(1.9 * 768.0, 768.0), 0.0);
    assert_eq!(
        cloud_distance_fade(5000.0, 0.0),
        1.0,
        "unset distance never fades"
    );
    assert_eq!(cloud_distance_fade(f32::NAN, 768.0), 0.0);
    let frame = AtmosphereFrame::default().with_cloud_fade_distance(768.0);
    assert_eq!(frame.cloud_fade_distance(), 768.0);
    assert_eq!(
        frame
            .with_cloud_fade_distance(f32::NAN)
            .cloud_fade_distance(),
        0.0
    );
}

#[test]
fn sun_and_moon_use_native_phase_gates_without_invented_horizon_fading() {
    let shader = include_str!("../src/atmosphere.wgsl");
    assert!(shader.contains("degrees <= 105.0 || degrees >= 255.0"));
    assert!(shader.contains("celestial_visibility(0.0)"));
    assert!(shader.contains("celestial_visibility(180.0)"));
    assert!(!shader.contains("smoothstep(-0.04, 0.02"));
}

#[test]
fn celestial_texels_are_composited_additively_without_an_rgb_opacity_key() {
    let shader = include_str!("../src/atmosphere.wgsl");
    assert!(
        shader.contains("fn composite_celestial("),
        "sun and moon must share one additive composition helper"
    );
    assert_eq!(
        shader.matches("composite_celestial(colour,").count(),
        2,
        "both sun and moon must use the shared additive helper"
    );
    assert!(shader.contains("return destination + sampled_rgb * coverage;"));
    assert!(!shader.contains("celestial_opacity"));
    assert!(!shader.contains("mix(colour, sun.rgb, sun.a)"));
    assert!(!shader.contains("mix(colour, moon.rgb, moon.a)"));
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
fn texture_backed_sky_shader_parses_validates_and_has_no_fullscreen_cloud_plane() {
    let shader = shader_source::standalone(include_str!("../src/atmosphere.wgsl"), &[]);
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
    assert!(shader.contains("@binding(4) var atmosphere_sampler: sampler;"));
    assert!(shader.contains("textureSampleLevel(sun_texture"));
    assert!(shader.contains("textureSampleLevel(moon_phases_texture"));
    assert!(!shader.contains("clouds_texture"));
    assert!(!shader.contains("textureSampleLevel(clouds_texture"));
    assert!(shader.contains("let phase_column = phase % 4u;"));
    assert!(shader.contains("let phase_row = phase / 4u;"));
    assert!(!shader.contains("sample_cloud_layer"));
    assert!(!shader.contains("mix(colour, cloud_colour, cloud_alpha)"));
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
fn dense_camera_medium_fog_replaces_the_infinite_sky_before_celestial_composition() {
    let shader = include_str!("../src/atmosphere.wgsl");
    let guard = "if (code / 4u != 0u)";
    let fog_return = "return vec4(atmosphere.fog_color_start.rgb, 1.0);";
    assert!(shader.contains(guard));
    assert!(shader.contains(fog_return));
    assert!(
        shader.find(guard).unwrap() < shader.find("let sun = sample_sun(").unwrap(),
        "medium fog must hide the infinite sky before sun/moon/cloud composition"
    );
}

#[test]
fn sky_shader_draws_native_stars_and_dimension_skies_without_a_sunrise_overlay() {
    let shader = include_str!("../src/atmosphere.wgsl");
    for needle in [
        "var<storage, read> stars: array<vec4<f32>>;",
        "if (kind == 1u)",
        "if (kind == 2u)",
        "sunrise_band: vec4<f32>",
        "sky_extra: vec4<f32>",
    ] {
        assert!(shader.contains(needle), "missing {needle}");
    }
    assert!(!shader.contains("fn sunrise_glow("));
    for (name, shader) in [
        ("chunk", include_str!("../src/chunk.wgsl")),
        ("model", include_str!("../src/model.wgsl")),
        ("liquid", include_str!("../src/liquid.wgsl")),
    ] {
        assert!(
            !shader.contains("smoothstep(\n        atmosphere.fog"),
            "{name} fog is linear"
        );
        assert!(
            !shader.contains("smoothstep(atmosphere.fog"),
            "{name} fog is linear"
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
#[allow(
    dead_code,
    reason = "shared shader adapter uses production material definitions"
)]
#[path = "../src/material_shader.rs"]
mod material_shader;
