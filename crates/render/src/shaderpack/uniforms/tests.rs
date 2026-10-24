use super::*;
use shaderpack::{ScalarType, StageMask, UniformMember};

fn member(name: &str, offset: u32, ty: ValueType, array: Option<(u32, u32)>) -> UniformMember {
    UniformMember {
        name: name.to_string(),
        offset,
        ty,
        array,
    }
}

fn read_f32(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn read_i32(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn camera() -> ViewCamera {
    ViewCamera {
        position: Vec3::new(10.5, 70.25, -3.75),
        rotation: Quat::from_rotation_y(0.3),
        focal: [1.2, 2.0],
        screen: [1600, 900],
    }
}

fn uniforms(game: &ShaderPackFrameInputs, history: &mut UniformHistory) -> FrameUniforms {
    FrameUniforms::new(
        camera(),
        game,
        &PackSettings::default(),
        FrameClock {
            delta_seconds: 0.25,
        },
        history,
    )
}

#[test]
fn members_pack_by_reflected_name_offset_and_type() {
    let float = ValueType::Scalar(ScalarType::Float);
    let int = ValueType::Scalar(ScalarType::Int);
    let vec3 = ValueType::Vector {
        size: 3,
        scalar: ScalarType::Float,
    };
    let ivec2 = ValueType::Vector {
        size: 2,
        scalar: ScalarType::Int,
    };
    let mat4 = ValueType::Matrix {
        columns: 4,
        rows: 4,
        scalar: ScalarType::Float,
    };
    let mat3 = ValueType::Matrix {
        columns: 3,
        rows: 3,
        scalar: ScalarType::Float,
    };
    let block = UniformBlock {
        name: "IrisUniforms".into(),
        group: 0,
        binding: 0,
        size: 400,
        members: vec![
            member("gbufferProjection", 0, mat4, None),
            member("cameraPosition", 64, vec3, None),
            member("frameTimeCounter", 76, float, None),
            member("eyeBrightness", 80, ivec2, None),
            member("worldTime", 88, int, None),
            member("viewWidth", 92, int, None),
            member("iris_NormalMat", 96, mat3, None),
            member("iris_TextureMat", 144, mat4, Some((2, 64))),
            member("someModUniform", 272, float, None),
        ],
        stages: StageMask::default(),
    };
    let game = ShaderPackFrameInputs {
        world_time: 13_000,
        eye_brightness: [32, 240],
        frame_time_counter: Some(5.5),
        ..ShaderPackFrameInputs::default()
    };
    let frame = uniforms(&game, &mut UniformHistory::default());
    let mut bytes = vec![];
    let mut unknown = BTreeSet::new();
    pack_block(&block, &|n| frame.value(n), &mut bytes, &mut unknown);
    assert_eq!(bytes.len(), 400);
    let projection = frame.projection();
    for column in 0..4 {
        for row in 0..4 {
            assert_eq!(
                read_f32(&bytes, column * 16 + row * 4),
                projection.col(column)[row]
            );
        }
    }
    assert_eq!(read_f32(&bytes, 64), 10.5);
    assert_eq!(read_f32(&bytes, 72), -3.75);
    assert_eq!(read_f32(&bytes, 76), 5.5, "a pinned frameTimeCounter");
    assert_eq!((read_i32(&bytes, 80), read_i32(&bytes, 84)), (32, 240));
    assert_eq!(read_i32(&bytes, 88), 13_000);
    assert_eq!(
        read_i32(&bytes, 92),
        1600,
        "float value converts to an int member"
    );
    assert_eq!(
        read_f32(&bytes, 96 + 16 + 4),
        1.0,
        "mat3 columns use a 16-byte stride"
    );
    assert_eq!(read_f32(&bytes, 96 + 12), 0.0);
    assert_eq!(
        read_f32(&bytes, 144 + 64 + 3 * 16 + 12),
        1.0,
        "every array element"
    );
    assert_eq!(read_f32(&bytes, 272), 0.0);
    assert_eq!(unknown.into_iter().collect::<Vec<_>>(), ["someModUniform"]);
}

#[test]
fn translated_boolean_uniforms_pack_scalars_vectors_arrays_and_grid_elements() {
    let translated = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
        (
            "composite.vsh",
            "#version 450\nvoid main() { gl_Position = vec4(0.0); }",
        ),
        (
            "composite.fsh",
            "#version 450
            uniform bool enabled;
            uniform bvec2 pairs[2];
            uniform bool flags[2][2];
            out vec4 color;
            void main() {
                color = vec4((enabled && pairs[1].x && flags[0][1]) ? 1.0 : 0.0);
            }",
        ),
    ]))
    .translate(&shaderpack::TranslateConfig::default());
    let program = translated
        .programs
        .iter()
        .find(|p| p.program.name == "composite")
        .unwrap();
    let reflected = program
        .reflection
        .as_ref()
        .unwrap_or_else(|| panic!("{:?}", program.all_diagnostics().collect::<Vec<_>>()));
    let block = reflected.uniforms.as_ref().unwrap();
    let mut bytes = vec![];
    let mut unknown = BTreeSet::new();
    pack_block(
        block,
        &|name| match name {
            "enabled" => Some(HostValue::from_custom(UniformValue::Bool(true))),
            "pairs[0]" => Some(HostValue::IVec([0, 1, 0, 0], 2)),
            "pairs[1]" => Some(HostValue::IVec([1, 0, 0, 0], 2)),
            "flags[0][0]" | "flags[1][1]" => Some(HostValue::bool(false)),
            "flags[0][1]" | "flags[1][0]" => Some(HostValue::bool(true)),
            _ => None,
        },
        &mut bytes,
        &mut unknown,
    );
    assert!(unknown.is_empty(), "{unknown:?}");
    assert_eq!(bytes.len(), block.size as usize);
    for (name, expected) in [
        ("enabled", vec![vec![1]]),
        ("pairs", vec![vec![0, 1], vec![1, 0]]),
        ("flags[0]", vec![vec![0], vec![1]]),
        ("flags[1]", vec![vec![1], vec![0]]),
    ] {
        let member = block
            .members
            .iter()
            .find(|member| member.name == name)
            .unwrap();
        let (count, stride) = member.array.unwrap_or((1, 0));
        assert_eq!(count as usize, expected.len());
        let width = expected[0].len();
        assert_eq!(
            member.ty,
            if width == 1 {
                ValueType::Scalar(ScalarType::Int)
            } else {
                ValueType::Vector {
                    size: width as u8,
                    scalar: ScalarType::Int,
                }
            }
        );
        for (element, expected) in expected.iter().enumerate() {
            let at = member.offset as usize + element * stride as usize;
            for (component, expected) in expected.iter().enumerate() {
                assert_eq!(
                    read_i32(&bytes, at + component * 4),
                    *expected,
                    "{name}[{element}]"
                );
            }
            if stride > 0 {
                assert!(
                    bytes[at + width * 4..at + stride as usize]
                        .iter()
                        .all(|byte| *byte == 0)
                );
            }
        }
    }
}

#[test]
fn gl_projection_maps_the_planes_to_clip_depth_limits() {
    let p = gl_perspective([1.0, 1.0], 0.05, 1024.0);
    let ndc = |distance: f32| {
        let clip = p * Vec4::new(0.0, 0.0, -distance, 1.0);
        clip.z / clip.w
    };
    assert!((ndc(0.05) + 1.0).abs() < 1e-4);
    assert!((ndc(1024.0) - 1.0).abs() < 1e-4);
    let o = gl_ortho(0.0, 1.0, 0.0, 1.0, -1.0, 1.0);
    assert_eq!(
        o * Vec4::new(0.0, 0.0, 0.0, 1.0),
        Vec4::new(-1.0, -1.0, 0.0, 1.0)
    );
    assert_eq!(
        o * Vec4::new(1.0, 1.0, 0.0, 1.0),
        Vec4::new(1.0, 1.0, 0.0, 1.0)
    );
}

#[test]
fn celestial_positions_follow_the_time_of_day_in_view_space() {
    let level = ViewCamera {
        rotation: Quat::IDENTITY,
        ..camera()
    };
    let at = |celestial_angle: f32| {
        let game = ShaderPackFrameInputs {
            celestial_angle,
            ..ShaderPackFrameInputs::default()
        };
        FrameUniforms::new(
            level,
            &game,
            &PackSettings::default(),
            FrameClock { delta_seconds: 0.0 },
            &mut UniformHistory::default(),
        )
    };
    let noon = at(0.0);
    let Some(HostValue::Vec(sun, 3)) = noon.value("sunPosition") else {
        panic!("sunPosition is a vec3");
    };
    assert!(
        (sun.y - 100.0).abs() < 1e-3,
        "the noon sun is overhead: {sun}"
    );
    assert_eq!(noon.value("sunAngle"), Some(HostValue::Float(0.25)));
    assert_eq!(noon.value("shadowLightPosition"), noon.value("sunPosition"));
    let midnight = at(0.5);
    assert_eq!(midnight.value("sunAngle"), Some(HostValue::Float(0.75)));
    assert_eq!(midnight.value("shadowAngle"), Some(HostValue::Float(0.25)));
    assert_eq!(
        midnight.value("shadowLightPosition"),
        midnight.value("moonPosition")
    );
    let sunrise = at(0.75);
    let Some(HostValue::Vec(sun, 3)) = sunrise.value("sunPosition") else {
        panic!("sunPosition is a vec3");
    };
    assert!(sun.x > 99.0, "the sun rises in the east (+X): {sun}");
    assert_eq!(
        sunrise.value("upPosition"),
        Some(HostValue::vec3(Vec3::Y * 100.0))
    );
}

#[test]
fn history_supplies_previous_matrices_and_smoothed_values() {
    let mut history = UniformHistory::default();
    let dry = ShaderPackFrameInputs::default();
    let first = uniforms(&dry, &mut history);
    assert_eq!(
        first.value("gbufferPreviousModelView"),
        first.value("gbufferModelView")
    );
    assert_eq!(first.value("frameCounter"), Some(HostValue::Int(0)));
    let raining = ShaderPackFrameInputs {
        rain_strength: 1.0,
        ..dry
    };
    let second = uniforms(&raining, &mut history);
    assert_eq!(second.value("frameCounter"), Some(HostValue::Int(1)));
    let Some(HostValue::Float(wetness)) = second.value("wetness") else {
        panic!("wetness is a float");
    };
    assert!(
        wetness > 0.0 && wetness < 0.1,
        "wetness rises slowly: {wetness}"
    );
    assert_eq!(second.value("rainStrength"), Some(HostValue::Float(1.0)));
    assert_eq!(
        second.value("previousCameraPosition"),
        first.value("cameraPosition")
    );
    assert_eq!(
        second.value("cameraPositionInt"),
        Some(HostValue::IVec([10, 70, -4, 0], 3))
    );
    assert_eq!(first.value("frameTimeCounter"), Some(HostValue::Float(0.0)));
    assert_eq!(
        second.value("frameTimeCounter"),
        Some(HostValue::Float(0.25)),
        "frameTimeCounter adds up frame times from the pack's load"
    );
}

#[test]
fn smoothed_uniforms_follow_iris_half_lives_in_tenths_of_a_second() {
    let mut history = UniformHistory::default();
    let settings = PackSettings::default();
    let step = |game: &ShaderPackFrameInputs, history: &mut UniformHistory, delta: f32| {
        FrameUniforms::new(
            camera(),
            game,
            &settings,
            FrameClock {
                delta_seconds: delta,
            },
            history,
        )
    };
    let indoors = ShaderPackFrameInputs {
        eye_brightness: [0, 0],
        center_distance: Some(2.0),
        ..ShaderPackFrameInputs::default()
    };
    let first = step(&indoors, &mut history, 0.0);
    assert_eq!(
        first.value("eyeBrightnessSmooth"),
        Some(HostValue::IVec([0, 0, 0, 0], 2))
    );
    let Some(HostValue::Float(near)) = first.value("centerDepthSmooth") else {
        panic!("centerDepthSmooth is a float");
    };
    let outdoors = ShaderPackFrameInputs {
        eye_brightness: [0, 240],
        center_distance: None,
        ..indoors
    };
    // eyeBrightnessHalflife 10 is one second: after one second half the way, truncated.
    let later = step(&outdoors, &mut history, 1.0);
    let Some(HostValue::IVec([0, sky, 0, 0], 2)) = later.value("eyeBrightnessSmooth") else {
        panic!("eyeBrightnessSmooth is an ivec2");
    };
    assert!((119..=120).contains(&sky), "half of 240, truncated: {sky}");
    // centerDepthHalflife 1 is a tenth of a second: a second later it has all but arrived.
    let Some(HostValue::Float(far)) = later.value("centerDepthSmooth") else {
        panic!("centerDepthSmooth is a float");
    };
    assert!(near > 0.9 && near < 1.0, "a block 2 away: {near}");
    assert!(far > 0.999 && far <= 1.0, "the sky reads depth 1: {far}");
    assert_eq!(later.value("frameTimeCounter"), Some(HostValue::Float(1.0)));
    history.frame_time_counter = 3599.5;
    let wrapped = step(&outdoors, &mut history, 1.0);
    assert_eq!(
        wrapped.value("frameTimeCounter"),
        Some(HostValue::Float(0.0)),
        "restarts from zero after an hour, as Iris does"
    );
}

#[test]
fn half_lives_and_shadow_planes_come_from_const_directives() {
    let consts = [
        ("wetnessHalflife", "70.0"),
        ("drynessHalflife", "300.0"),
        ("eyeBrightnessHalflife", "5.0f"),
        ("shadowNearPlane", "-50.0"),
        ("shadowFarPlane", "-1.0"),
    ]
    .map(|(name, value)| shaderpack::ConstDirective {
        ty: "float".into(),
        name: name.into(),
        value: value.into(),
    });
    let settings = PackSettings::read(consts.iter());
    assert!(
        (settings.wetness_halflife - 30.0).abs() < 1e-4,
        "Iris lets drynessHalflife set the wetness half-life"
    );
    assert!((settings.dryness_halflife - 20.0).abs() < 1e-4);
    assert!((settings.eye_brightness_halflife - 0.5).abs() < 1e-5);
    assert_eq!(settings.shadow_near_plane, -50.0);
    let frame = at_time(0.1, settings, Vec3::ZERO);
    let projection = frame.shadow_projection();
    let far = projection * Vec4::new(0.0, 0.0, -256.0, 1.0);
    assert!(
        (far.z - 1.0).abs() < 1e-4,
        "-1 is the render distance: {far}"
    );
}

fn at_time(celestial_angle: f32, settings: PackSettings, position: Vec3) -> FrameUniforms {
    let game = ShaderPackFrameInputs {
        celestial_angle,
        ..ShaderPackFrameInputs::default()
    };
    FrameUniforms::new(
        ViewCamera {
            position,
            ..camera()
        },
        &game,
        &settings,
        FrameClock { delta_seconds: 0.0 },
        &mut UniformHistory::default(),
    )
}

#[test]
fn the_shadow_camera_looks_from_the_shadow_light() {
    for sun_path_rotation in [0.0, 25.0] {
        let settings = PackSettings {
            sun_path_rotation,
            ..PackSettings::default()
        };
        for angle in [0.0, 0.1, 0.2, 0.3, 0.6, 0.8, 0.95] {
            let frame = at_time(angle, settings, Vec3::new(0.25, 64.0, -7.5));
            let Some(HostValue::Vec(light, 3)) = frame.value("shadowLightPosition") else {
                panic!("shadowLightPosition is a vec3");
            };
            let world = frame
                .model_view()
                .inverse()
                .transform_vector3(light.truncate())
                .normalize();
            let towards = frame.shadow_model_view().transform_vector3(world);
            assert!(
                (towards - Vec3::Z).length() < 1e-3,
                "angle {angle}, rotation {sun_path_rotation}: light maps to {towards}"
            );
        }
    }
}

#[test]
fn the_shadow_camera_snaps_to_the_interval_and_spans_the_shadow_distance() {
    let settings = PackSettings {
        shadow: ShadowCullSettings {
            shadow_distance: 120.0,
            ..Default::default()
        },
        shadow_interval: 4.0,
        ..PackSettings::default()
    };
    let snapped = |x: f32| {
        let frame = at_time(0.1, settings, Vec3::new(x, 70.0, 0.0));
        // A fixed world point, relative to the camera, in shadow view space.
        frame
            .shadow_model_view()
            .transform_point3(Vec3::new(10.0 - x, -70.0, 0.0))
    };
    assert!(
        (snapped(1.0) - snapped(1.5)).length() < 1e-4,
        "camera moves within an interval keep the shadow map still"
    );
    assert!(
        (snapped(1.0) - snapped(5.0)).length() > 3.9,
        "a whole interval moves it"
    );
    let frame = at_time(0.1, settings, Vec3::ZERO);
    let projection = frame.shadow_projection();
    let edge = projection * Vec4::new(120.0, -120.0, 100.05, 1.0);
    assert!((edge.x - 1.0).abs() < 1e-5 && (edge.y + 1.0).abs() < 1e-5);
    assert!(
        (edge.z + 1.0).abs() < 1e-4,
        "near plane 100 blocks towards the light"
    );
    let far = projection * Vec4::new(0.0, 0.0, -156.0, 1.0);
    assert!((far.z - 1.0).abs() < 1e-4, "far plane 156 blocks away");
}

#[test]
fn draw_uniforms_fold_the_chunk_offset_into_the_legacy_model_view() {
    let draw = DrawUniforms {
        model_view: Mat4::from_rotation_y(0.5),
        projection: gl_perspective([1.0, 1.0], 0.05, 100.0),
        chunk_offset: Vec3::new(-3.0, 1.0, 2.0),
        render_stage: 8,
        lightmap_matrix: true,
        atlas_size: [512, 256],
        entity_id: -1,
    };
    let Some(HostValue::Mat4(legacy)) = draw.value("iris_ModelViewMat") else {
        panic!("legacy model-view is a matrix");
    };
    let vertex = Vec3::new(1.0, 2.0, 3.0);
    assert!(
        (legacy.transform_point3(vertex)
            - draw.model_view.transform_point3(vertex + draw.chunk_offset))
        .length()
            < 1e-5
    );
    assert_eq!(
        draw.value("modelViewMatrix"),
        Some(HostValue::Mat4(draw.model_view))
    );
    assert_eq!(
        draw.value("iris_TextureMat[1]"),
        Some(HostValue::Mat4(
            super::super::geometry::LIGHTMAP_TEXTURE_MATRIX
        ))
    );
    assert_eq!(
        draw.value("iris_TextureMat[0]"),
        None,
        "the frame's identity"
    );
    assert_eq!(draw.value("renderStage"), Some(HostValue::Int(8)));
}

#[test]
fn custom_uniform_values_round_trip_through_host_values() {
    for value in [
        UniformValue::Float(1.5),
        UniformValue::Int(-3),
        UniformValue::Vec2([1.0, 2.0]),
        UniformValue::Vec3([1.0, 2.0, 3.0]),
        UniformValue::Vec4([1.0, 2.0, 3.0, 4.0]),
    ] {
        assert_eq!(HostValue::from_custom(value).to_custom(), Some(value));
    }
    assert_eq!(
        HostValue::from_custom(UniformValue::Bool(true)),
        HostValue::Int(1)
    );
    assert_eq!(HostValue::Mat4(Mat4::IDENTITY).to_custom(), None);
}

/// `bedrockLevel` is the dimension's lowest block Y, as Iris reads `dimensionType().minY()`.
#[test]
fn bedrock_level_is_the_dimension_minimum_y() {
    let game = ShaderPackFrameInputs {
        bedrock_level: -64,
        ..ShaderPackFrameInputs::default()
    };
    let frame = uniforms(&game, &mut UniformHistory::default());
    assert_eq!(frame.value("bedrockLevel"), Some(HostValue::Int(-64)));
}

#[test]
fn biome_uniforms_read_the_java_biome_of_the_bedrock_biome() {
    let Ok(mapping) = JavaBlockMapping::pinned() else {
        eprintln!("skipping: missing Java mapping carrier fixture");
        return;
    };
    let desert = (0..u32::from(u16::MAX))
        .find(|id| mapping.java_biome(*id) == Some("minecraft:desert"))
        .unwrap();
    let game = ShaderPackFrameInputs {
        biome: Some(desert),
        ..ShaderPackFrameInputs::default()
    };
    let frame = uniforms(&game, &mut UniformHistory::default());
    assert_eq!(frame.value("temperature"), Some(HostValue::Float(2.0)));
    assert_eq!(frame.value("rainfall"), Some(HostValue::Float(0.0)));
    assert_eq!(frame.value("biome_precipitation"), Some(HostValue::Int(0)));
    assert_eq!(
        frame.value("biome_category"),
        Some(HostValue::Int(shaderpack::BiomeCategory::Desert.ordinal()))
    );
    let before_world = uniforms(
        &ShaderPackFrameInputs::default(),
        &mut UniformHistory::default(),
    );
    assert_eq!(
        before_world.value("biome_category"),
        Some(HostValue::Int(0))
    );
    assert_eq!(
        before_world.value("temperature"),
        Some(HostValue::Float(0.0))
    );
}

#[test]
fn sky_and_fog_colours_are_javas_for_the_blended_biomes() {
    let Ok(mapping) = JavaBlockMapping::pinned() else {
        eprintln!("skipping: missing Java mapping carrier fixture");
        return;
    };
    let plains = (0..u32::from(u16::MAX))
        .find(|id| mapping.java_biome(*id) == Some("minecraft:plains"))
        .unwrap();
    let mut sky_biomes = [(0, 0.0); render_scene::shaderpack::SKY_BIOME_SLOTS];
    sky_biomes[0] = (plains, 1.0);
    let game = ShaderPackFrameInputs {
        celestial_angle: 0.0,
        render_distance: 512.0,
        sky_biomes,
        ..ShaderPackFrameInputs::default()
    };
    let frame = uniforms(&game, &mut UniformHistory::default());
    let rgb = |hex: u32| {
        Vec3::new(
            ((hex >> 16) & 0xFF) as f32 / 255.0,
            ((hex >> 8) & 0xFF) as f32 / 255.0,
            (hex & 0xFF) as f32 / 255.0,
        )
    };
    assert_eq!(
        frame.value("skyColor"),
        Some(HostValue::vec3(rgb(0x78_A7_FF)))
    );
    let Some(HostValue::Vec(fog, 3)) = frame.value("fogColor") else {
        panic!("fogColor is a vec3");
    };
    assert!(
        (fog.truncate() - rgb(0xC0_D8_FF)).length() < 1e-3,
        "noon fog at a 32-chunk render distance is the biome fog: {fog}"
    );
}
