use meshing::{CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y, CloudFace, PackedCloudQuad};

fn substitute_test_view(shader: &str) -> String {
    shader.replacen(
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
    )
}

#[test]
fn finite_cloud_shader_parses_validates_and_vertex_pulls_nine_periods() {
    let shader = substitute_test_view(include_str!("../src/cloud.wgsl"));
    let module = naga::front::wgsl::parse_str(&shader).expect("parse cloud WGSL");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator.validate(&module).expect("validate cloud WGSL");

    assert!(shader.contains("let quad_index = vertex_index / 6u;"));
    assert!(shader.contains("let corner_index = vertex_index % 6u;"));
    assert!(shader.contains("cloud_records[quad_index]"));
    assert!(shader.contains("let instance_column = i32(instance_index % 3u) - 1;"));
    assert!(shader.contains("let instance_row = i32(instance_index / 3u) - 1;"));
    assert!(shader.contains("atmosphere.fog_end_time.z * CLOUD_TEXTURE_WORLD_PERIOD"));
    assert!(shader.contains("view.clip_from_world * vec4(world_position, 1.0)"));
    assert!(!shader.contains("position.z = 0.0"));
}

#[test]
fn all_six_faces_reconstruct_the_packed_fixed_height_bounds() {
    assert_eq!(CLOUD_UNDERSIDE_Y, 192.33);
    assert_eq!(CLOUD_TOP_Y, 196.33);
    let shader = include_str!("../src/cloud.wgsl");
    assert!(shader.contains("const CLOUD_UNDERSIDE_Y: f32 = 192.33;"));
    assert!(shader.contains("const CLOUD_TOP_Y: f32 = 196.33;"));
    assert!(shader.contains("const CLOUD_CELL_BLOCKS: f32 = 16.0;"));
    assert!(shader.contains("const CLOUD_TEXTURE_WORLD_PERIOD: f32 = 4096.0;"));
    assert!(shader.contains("local_position.x * CLOUD_CELL_BLOCKS + instance_origin.x"));

    for (face, mapping) in [
        (CloudFace::Down, "vec3(x, CLOUD_UNDERSIDE_Y, z)"),
        (CloudFace::Up, "vec3(x, CLOUD_TOP_Y, z)"),
        (CloudFace::North, "vec3(run, y, axis1_start)"),
        (CloudFace::South, "vec3(run, y, axis1_start)"),
        (CloudFace::West, "vec3(axis1_start, y, run)"),
        (CloudFace::East, "vec3(axis1_start, y, run)"),
    ] {
        let packed = PackedCloudQuad::try_pack(7, 11, 5, 4, face).unwrap();
        assert_eq!(packed.face(), face);
        assert!(
            shader.contains(mapping),
            "missing {face:?} mapping through {mapping}"
        );
    }
    for branch in [
        "face == FACE_DOWN",
        "face == FACE_UP",
        "face == FACE_NORTH",
        "face == FACE_SOUTH",
        "face == FACE_WEST",
    ] {
        assert!(
            shader.contains(branch),
            "missing reconstruction branch {branch}"
        );
    }
}

#[test]
fn one_sorted_item_draws_exact_quad_vertices_and_nine_instances() {
    let source = include_str!("../src/cloud_render.rs");
    assert!(source.contains("PhaseItemExtraIndex::None"));
    assert!(source.contains("cloud_phase_distance("));
    assert!(!source.contains("mesh::Mesh"));
    assert!(!source.contains("AssetId::<Mesh>"));
    assert!(!source.contains("StandardMaterial"));
    assert!(source.contains("if gpu.record_count == 0"));
    assert!(source.contains("let vertex_count = gpu.record_count.checked_mul(6)"));
    assert!(source.contains("pass.draw(0..vertex_count, 0..9);"));
    assert_eq!(source.matches("pass.draw(").count(), 1);
}

#[test]
fn cloud_fragment_uses_baked_shade_vanilla_colour_and_distance_fade_without_fog() {
    let shader = include_str!("../src/cloud.wgsl").replace("\r\n", "\n");
    assert!(shader.contains("const RAIN_CLOUD_COLOUR: vec3<f32> = vec3(191.0 / 255.0);"));
    assert!(shader.contains("const THUNDER_CLOUD_COLOUR: vec3<f32> = vec3(30.0 / 255.0);"));
    assert!(shader.contains("const WEATHER_COLOUR_CONTRIBUTION: f32 = 0.95;"));
    assert!(shader.contains("const CLOUD_ALPHA: f32 = 0.7;"));
    assert!(shader.contains("const CLOUD_FADE_START: f32 = 0.9;"));
    assert!(shader.contains("const CLOUD_SUNRISE_WEIGHT: f32 = 0.35;"));
    assert!(shader.contains("cos(TAU * atmosphere.sky_extra.y)"));
    assert!(shader.contains("atmosphere.fog_end_time.w"));
    assert!(shader.contains("cloud_colour() * face_shade(in.normal)"));
    assert!(!shader.contains("dot(in.normal"));
    assert!(!shader.contains("fog_color_start.rgb"));
    assert!(!shader.contains("textureSample"));
}

#[test]
fn shader_period_selection_matches_euclidean_negative_coordinate_semantics() {
    let shader = include_str!("../src/cloud.wgsl");
    assert!(shader.contains(
        "floor((view.world_position.x - cloud_texture_offset) / CLOUD_TEXTURE_WORLD_PERIOD)"
    ));
    assert!(shader.contains("floor(view.world_position.z / CLOUD_TEXTURE_WORLD_PERIOD)"));
    assert!(shader.contains("let instance_column = i32(instance_index % 3u) - 1;"));
    assert!(shader.contains("let instance_row = i32(instance_index / 3u) - 1;"));
}
