use crate::material_shader;
use crate::shader_source;

#[test]
fn native_leaf_face_policy_uses_the_carrier_flag_in_both_colour_and_depth() {
    let material = material_shader::source(include_str!("../../src/material.wgsl"));
    assert!(material.contains(&format!(
        "const TWO_SIDED: u32 = {}u;",
        assets::MATERIAL_FLAG_TWO_SIDED
    )));
    assert!(material.contains("return front || (flags & TWO_SIDED) != 0u;"));
    let chunk = include_str!("../../src/chunk.wgsl");
    assert_eq!(
        chunk.matches("@builtin(front_facing) front: bool").count(),
        2
    );
    assert_eq!(
        chunk
            .matches("if (!material_face_is_visible(in.material_flags, front)) { discard; }")
            .count(),
        2
    );
    // Installed native RenderChunk AlphaTest passes (1.26.51.01 Metal) test
    // alpha at .5. SeasonsOn writes opaque alpha; SeasonsOff preserves the
    // sampled alpha. This test does not claim native MSAA/A2C coverage parity.
    // Deep leaves have no cutout flag.
    for definitions in [&[][..], &["ENHANCED_SHADOW"][..]] {
        let source = shader_source::standalone(chunk, definitions);
        assert_eq!(
            shader_source::alpha_discard_threshold(&source, "fragment"),
            Some(0.5)
        );
        if definitions.contains(&"ENHANCED_SHADOW") {
            assert_eq!(
                shader_source::alpha_discard_threshold(&source, "fragment_shadow"),
                Some(0.5)
            );
        }
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
    let pipeline = include_str!("../../src/chunk/pipeline/layouts.rs");
    let cube = pipeline.split("let mut model_descriptor").next().unwrap();
    assert!(cube.contains("cull_mode: None"));
    assert!(cube.contains("depth_write_enabled: true"));
}

#[test]
fn native_leaf_colour_is_world_material_gated_and_enhanced_keeps_its_existing_path() {
    let material = material_shader::source(include_str!("../../src/material.wgsl"));
    assert!(material.contains(&format!(
        "const NATIVE_LEAF_COLOUR: u32 = {}u;",
        assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR
    )));
    assert!(material.contains("return (flags & NATIVE_LEAF_COLOUR) != 0u;"));
    let chunk = include_str!("../../src/chunk.wgsl");
    let ordinary = shader_source::preprocess(chunk, &[]);
    let enhanced = shader_source::preprocess(chunk, &["ENHANCED"]);
    assert!(ordinary.contains("let native_colour = native_cube_colour("));
    assert!(ordinary.contains("out.native_light_levels = terrain_light_levels(light_sample);"));
    assert!(ordinary.contains("if (tint_kind != 0u)"));
    assert!(!enhanced.contains("let native_colour = native_cube_colour("));
    assert!(!enhanced.contains("out.native_light_levels ="));
    assert!(enhanced.contains("let shaded = shade_surface("));
    assert!(ordinary.contains("textureSampleGrad(native_leaf_textures_page_0"));
    assert!(ordinary.contains("textureSampleGrad(native_leaf_textures_page_1"));
    assert!(!enhanced.contains("textureSampleGrad(native_leaf_textures_page_"));
    assert!(!ordinary.contains("tint_to_gamma(vec4(texture_gamma"));
}

#[test]
fn alternate_atlas_views_and_native_sampler_fit_baseline_limits_without_more_storage() {
    let baseline = wgpu::Limits::default();
    assert!(material_shader::chunk_atlas_views_fit(&baseline));
    let required = material_shader::CHUNK_SAMPLED_TEXTURE_BINDINGS;
    let mut limits = baseline;
    limits.max_sampled_textures_per_shader_stage = required - 1;
    assert!(!material_shader::chunk_atlas_views_fit(&limits));
    limits.max_sampled_textures_per_shader_stage = required;
    assert!(material_shader::chunk_atlas_views_fit(&limits));
    limits.max_bindings_per_bind_group = material_shader::NATIVE_LEAF_SAMPLER_BINDING;
    assert!(!material_shader::chunk_atlas_views_fit(&limits));
    limits.max_bindings_per_bind_group += 1;
    assert!(material_shader::chunk_atlas_views_fit(&limits));
    limits.max_samplers_per_shader_stage = 1;
    assert!(!material_shader::chunk_atlas_views_fit(&limits));
    let sampler = material_shader::native_leaf_sampler_descriptor();
    assert_eq!(sampler.min_filter, wgpu::FilterMode::Nearest);
    assert_eq!(sampler.mag_filter, wgpu::FilterMode::Nearest);
    assert_eq!(sampler.mipmap_filter, wgpu::FilterMode::Linear);
    assert_eq!(sampler.address_mode_u, wgpu::AddressMode::ClampToEdge);
    assert_eq!(sampler.address_mode_v, wgpu::AddressMode::ClampToEdge);
    assert_eq!(sampler.address_mode_w, wgpu::AddressMode::ClampToEdge);
    let uploader = include_str!("../../src/chunk/gpu/bind_groups.rs");
    assert!(uploader.contains("view_formats: &[TextureFormat::Rgba8Unorm]"));
    assert!(uploader.contains("format: TextureFormat::Rgba8UnormSrgb"));
    assert!(uploader.contains("let native_leaf_views = [&texture_0, &texture_1].map"));
    assert_eq!(uploader.matches("render_device.create_texture(").count(), 1);
    assert!(uploader.contains("crate::material_shader::native_leaf_sampler_descriptor()"));
}
