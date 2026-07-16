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
    assert_eq!(CLOUD_UNDERSIDE_Y, 128.0);
    assert_eq!(CLOUD_TOP_Y, 132.0);
    let shader = include_str!("../src/cloud.wgsl");
    assert!(shader.contains("const CLOUD_UNDERSIDE_Y: f32 = 128.0;"));
    assert!(shader.contains("const CLOUD_TOP_Y: f32 = 132.0;"));

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
fn cloud_pipeline_is_transparent_depth_aware_and_specializes_from_each_view() {
    let source = include_str!("../src/cloud_render.rs");
    assert!(source.contains("struct CloudPipelineKey"));
    assert!(source.contains("msaa: Msaa"));
    assert!(source.contains("hdr: bool"));
    assert!(source.contains("descriptor.multisample.count = key.msaa.samples()"));
    assert!(source.contains("ViewTarget::TEXTURE_FORMAT_HDR"));
    assert!(source.contains(".add_render_command::<Transparent3d, DrawCloudCommands>()"));
    assert!(source.contains("ViewSortedRenderPhases<Transparent3d>"));
    assert!(source.contains("phase.add(Transparent3d {"));
    assert!(source.contains("blend: Some(BlendState::ALPHA_BLENDING)"));
    assert!(source.contains("depth_write_enabled: false"));
    assert!(source.contains("depth_compare: CompareFunction::GreaterEqual"));
    assert!(source.contains("CORE_3D_DEPTH_FORMAT"));
    assert!(source.contains("BufferBindingType::Storage { read_only: true }"));
    assert!(source.contains("ShaderStages::VERTEX | ShaderStages::FRAGMENT"));
    assert!(!source.contains("BinnedRenderPhaseType::NonMesh"));
}

#[test]
fn cloud_gpu_resources_are_immutable_identity_cached_and_not_frame_rebuilt() {
    let source = include_str!("../src/cloud_render.rs");
    assert!(source.contains("pub(crate) struct CloudGpu"));
    assert!(source.contains("BufferUsages::STORAGE | BufferUsages::COPY_DST"));
    assert!(source.contains("prepared_identity == Some(identity)"));
    assert!(source.contains("bound_asset_identity == Some(identity)"));
    assert_eq!(source.matches("create_buffer_with_data(").count(), 1);
    assert_eq!(source.matches("create_bind_group(").count(), 1);
    assert!(!source.contains("write_buffer("));
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
fn cloud_fragment_uses_face_lighting_weather_and_bounded_distance_fog() {
    let shader = include_str!("../src/cloud.wgsl");
    assert!(shader.contains("const RAIN_CLOUD_COLOUR: vec3<f32> = vec3(191.0 / 255.0);"));
    assert!(shader.contains("const THUNDER_CLOUD_COLOUR: vec3<f32> = vec3(30.0 / 255.0);"));
    assert!(shader.contains("const WEATHER_COLOUR_CONTRIBUTION: f32 = 0.95;"));
    assert!(shader.contains("fn face_normal(face: u32) -> vec3<f32>"));
    assert!(shader.contains("dot(in.normal, sun_direction)"));
    assert!(shader.contains("atmosphere.sun_direction_daylight.xyz"));
    assert!(shader.contains("atmosphere.sun_direction_daylight.w"));
    assert!(shader.contains("atmosphere.sky_zenith_rain.w"));
    assert!(shader.contains("atmosphere.sky_horizon_thunder.w"));
    assert!(shader.contains("atmosphere.fog_color_start.w"));
    assert!(shader.contains("atmosphere.fog_end_time.x"));
    assert!(shader.contains("distance(in.world_position, view.world_position)"));
    assert!(shader.contains("fn bounded_cloud_fog("));
    assert!(shader.contains("fn invalid_cloud_fog_input(value: f32) -> bool"));
    assert!(shader.contains("return (bitcast<u32>(value) & 0x7f800000u) == 0x7f800000u;"));
    assert!(shader.contains("if (bounded_end <= bounded_start)"));
    assert!(shader.contains("select(0.0, 1.0, bounded_distance >= bounded_end)"));
    assert!(shader.contains("mix(cloud_colour, atmosphere.fog_color_start.rgb, fog)"));
    assert!(shader.contains("let cloud_alpha = clamp(1.0 - fog, 0.0, 1.0);"));
    assert!(shader.contains("return vec4(fogged_colour, cloud_alpha);"));
    assert!(!shader.contains("const SIDE_LIGHT"));
    assert!(!shader.contains("const UNDERSIDE_LIGHT"));
    assert!(!shader.contains("textureSample"));
    assert!(!shader.contains("sampled.a"));
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
