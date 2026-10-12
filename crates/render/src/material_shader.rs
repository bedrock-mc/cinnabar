//! The material carrier and WGSL consume the same flag discriminants.

pub(crate) const NATIVE_LEAF_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [16, 17];
pub(crate) const NATIVE_LEAF_SAMPLER_BINDING: u32 = 18;
pub(crate) const PBR_NORMAL_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [32, 33];
pub(crate) const PBR_MER_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [21, 22];
pub(crate) const PBR_SAMPLER_BINDING: u32 = 23;
pub(crate) const ENHANCED_COLOR_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [24, 25];
pub(crate) const ENHANCED_NORMAL_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [26, 27];
pub(crate) const ENHANCED_MER_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [28, 29];
pub(crate) const ENHANCED_SAMPLER_BINDING: u32 = 30;
pub(crate) const ENHANCED_TEXTURE_REF_BINDING: u32 = 31;
pub(crate) const LAST_CHUNK_BINDING: u32 =
    PBR_NORMAL_TEXTURE_BINDINGS[assets::MAX_TEXTURE_PAGES - 1];
pub(crate) const ACTIVE_LAST_CHUNK_BINDING: u32 = if cfg!(feature = "enhanced") {
    LAST_CHUNK_BINDING
} else {
    BIOME_QUERY_TABLES_BINDING
};
pub(crate) const CHUNK_SAMPLER_COUNT: u32 = 2 + if cfg!(feature = "enhanced") { 2 } else { 0 };
pub(crate) const CHUNK_SAMPLED_TEXTURE_BINDINGS: u32 = (assets::MAX_TEXTURE_PAGES
    + NATIVE_LEAF_TEXTURE_BINDINGS.len()
    + if cfg!(feature = "enhanced") {
        PBR_NORMAL_TEXTURE_BINDINGS.len()
            + PBR_MER_TEXTURE_BINDINGS.len()
            + ENHANCED_COLOR_TEXTURE_BINDINGS.len()
            + ENHANCED_NORMAL_TEXTURE_BINDINGS.len()
            + ENHANCED_MER_TEXTURE_BINDINGS.len()
    } else {
        0
    }) as u32;
pub(crate) const BIOME_QUERY_TABLES_BINDING: u32 = 19;
pub(crate) const WORLD_CLOCK_WORDS: usize = 4;
pub(crate) const WORLD_UNIFORM_BYTES: u64 =
    ((WORLD_CLOCK_WORDS + meshing::biome_lattice::BIOME_QUERY_TABLE_WORDS) * 4) as u64;

// Admitted source sizes are powers of two up to MAX_TILE_SIZE. Four bits store
// log2(size) + 1; zero means the physical page size. Grid occupies five more bits.
const TEXTURE_SIZE_BITS: u32 = 4;
const TEXTURE_WIDTH_SHIFT: u32 = assets::TEXTURE_LAYER_MASK.count_ones();
const TEXTURE_HEIGHT_SHIFT: u32 = TEXTURE_WIDTH_SHIFT + TEXTURE_SIZE_BITS;
const TEXTURE_GRID_SHIFT: u32 = TEXTURE_HEIGHT_SHIFT + TEXTURE_SIZE_BITS;
const TEXTURE_SIZE_EXPONENT_MASK: u32 = (1 << TEXTURE_SIZE_BITS) - 1;

/// Encodes admitted power-of-two dimensions above the shared page and layer fields.
pub(crate) fn gpu_texture_ref(
    reference: assets::TextureRef,
    dimensions: [u32; 2],
    page_size: u32,
) -> u32 {
    if dimensions == [page_size; 2] {
        reference.raw()
    } else {
        debug_assert!(
            dimensions
                .iter()
                .all(|size| size.is_power_of_two() && *size <= assets::texture::MAX_TILE_SIZE)
        );
        reference.raw()
            | ((dimensions[0].ilog2() + 1) << TEXTURE_WIDTH_SHIFT)
            | ((dimensions[1].ilog2() + 1) << TEXTURE_HEIGHT_SHIFT)
    }
}

/// Adds the terrain entry's UV-grid exponent without changing dimensions, page or layer.
pub(crate) fn gpu_grid_texture_ref(
    reference: assets::TextureRef,
    dimensions: [u32; 2],
    page_size: u32,
    grid: u8,
) -> u32 {
    gpu_texture_ref(reference, dimensions, page_size) | (u32::from(grid) << TEXTURE_GRID_SHIFT)
}

/// Packs the animation clock before the immutable biome lookup tables for one shared uniform.
pub(crate) fn world_uniform_words(clock: &[u32]) -> Vec<u32> {
    assert_eq!(clock.len(), WORLD_CLOCK_WORDS);
    let mut words = Vec::with_capacity(WORLD_UNIFORM_BYTES as usize / 4);
    words.extend_from_slice(clock);
    words.extend(meshing::biome_lattice::query_table_words());
    words
}

/// Resolves the generated biome tint module's table binding, shared by every chunk shader.
pub(crate) fn bind_biome_tables(source: &str) -> String {
    source.replace(
        "BIOME_QUERY_TABLES_BINDING",
        &BIOME_QUERY_TABLES_BINDING.to_string(),
    )
}

pub(crate) fn chunk_atlas_views_fit(limits: &wgpu::Limits) -> bool {
    limits.max_sampled_textures_per_shader_stage >= CHUNK_SAMPLED_TEXTURE_BINDINGS
        && limits.max_samplers_per_shader_stage >= CHUNK_SAMPLER_COUNT
        && limits.max_bindings_per_bind_group > ACTIVE_LAST_CHUNK_BINDING
}

/// Terrain samples nearest texels and interpolates only between mip levels.
pub(crate) fn native_leaf_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("native terrain leaf sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        min_filter: wgpu::FilterMode::Nearest,
        mag_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        lod_max_clamp: (assets::texture::VANILLA_TERRAIN_MIP_COUNT - 1) as f32,
        ..Default::default()
    }
}

/// Filters authored linear maps across texels, mips, and oblique surfaces.
#[cfg(feature = "enhanced")]
pub(crate) fn pbr_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("enhanced linear PBR sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    }
}

/// Formats the shared enchantment tint for the actor and UI shaders.
pub(crate) fn glint_tint_source() -> String {
    let [red, green, blue] = render_model::ActorGlint::TINT_RGB;
    format!("vec3<f32>({red:?}, {green:?}, {blue:?})")
}

pub(crate) fn source(source: &str) -> String {
    source
        .replace("// ENHANCED_PBR_LOOKUP", include_str!("enhanced/authored_lookup.wesl"))
        .replace("GLINT_TINT_RGB", &glint_tint_source())
        .replace("ACTOR_LIGHT_WORLD_FLAG", &format!("{}u", render_api::ACTOR_LIGHT_WORLD))
        .replace("ACTOR_LIGHT_DIRECTIONAL_FLAG", &format!("{}u", render_api::ACTOR_LIGHT_DIRECTIONAL))
        .replace("TEXTURE_LAYER_MASK", &format!("{}u", assets::TEXTURE_LAYER_MASK))
        .replace("GPU_TEXTURE_GRID_SHIFT", &format!("{TEXTURE_GRID_SHIFT}u"))
        .replace("GPU_TEXTURE_SIZE_EXPONENT_MASK", &format!("{TEXTURE_SIZE_EXPONENT_MASK}u"))
        .replace("TERRAIN_QUAD_SHIFT_MASK", &format!("{}u", assets::texture::TERRAIN_QUAD_SHIFT_MASK))
        .replace("GPU_TEXTURE_WIDTH_SHIFT", &format!("{TEXTURE_WIDTH_SHIFT}u"))
        .replace("GPU_TEXTURE_HEIGHT_SHIFT", &format!("{TEXTURE_HEIGHT_SHIFT}u"))
        .replace("BLOCK_OVERLAY_FACE_OFFSET", &format!("{:?}", render_api::BLOCK_OVERLAY_FACE_OFFSET))
        .replace("// ENHANCED_PBR_SAMPLING", include_str!("enhanced/pbr_sampling.wesl"))
        .replace("// ENHANCED_PBR_CONSTANTS", &format!(
            "const AUTHORED_REFERENCE_BASE_WORDS:u32={}u;\nconst AUTHORED_REFERENCE_OVERRIDE_WORDS:u32={}u;\nconst PBR_REF_COLOR:u32={}u;\nconst PBR_REF_NORMAL:u32={}u;\nconst PBR_REF_HEIGHT:u32={}u;\nconst PBR_REF_MATERIAL:u32={}u;\nconst PBR_REF_LABPBR:u32={}u;\nconst PBR_REF_OCCLUSION:u32={}u;\nconst PBR_REF_SUBSURFACE:u32={}u;\nconst PBR_HEIGHT_SCALE:f32={:?};",
            render_scene::EnhancedTextureAssets::REFERENCE_BASE_WORDS, render_scene::EnhancedTextureAssets::REFERENCE_OVERRIDE_WORDS,
            assets::texture::PBR_REF_COLOR, assets::texture::PBR_REF_NORMAL, assets::texture::PBR_REF_HEIGHT, assets::texture::PBR_REF_MATERIAL,
            assets::texture::PBR_REF_LABPBR, assets::texture::PBR_REF_OCCLUSION, assets::texture::PBR_REF_SUBSURFACE, assets::texture::PBR_HEIGHT_SCALE))
        .replace("ACTOR_MATERIAL_GLINT", &format!("{}u", assets::EntityRenderMaterial::Glint as u32))
        .replace("ACTOR_MATERIAL_DEFAULT", &format!("{}u", assets::EntityRenderMaterial::Default as u32))
        .replace("ACTOR_MATERIAL_DRAGON", &format!("{}u", assets::EntityRenderMaterial::Dragon as u32))
        .replace("ACTOR_MATERIAL_DISSOLVE_DEPTH", &format!("{}u", assets::EntityRenderMaterial::DissolveDepth as u32))
        .replace("ACTOR_MATERIAL_DISSOLVE_COLOR", &format!("{}u", assets::EntityRenderMaterial::DissolveColor as u32))
        .replace("ACTOR_MATERIAL_KIND_MASK", &format!("{}u", assets::EntityRenderMaterialState::KIND_MASK))
        .replace("ACTOR_MATERIAL_AUTHORED_FLAG", &format!("{}u", assets::EntityRenderMaterialState::AUTHORED))
        .replace("ACTOR_MATERIAL_ALPHA_TEST_FLAG", &format!("{}u", assets::EntityRenderMaterialState::ALPHA_TEST))
        .replace("ACTOR_MATERIAL_CULL_FLAG", &format!("{}u", assets::EntityRenderMaterialState::CULL))
        .replace("ACTOR_MATERIAL_BLEND_FLAG", &format!("{}u", assets::EntityRenderMaterialState::BLEND))
        .replace("ACTOR_MATERIAL_EMISSIVE_FLAG", &format!("{}u", assets::EntityRenderMaterialState::EMISSIVE))
        .replace("ACTOR_MATERIAL_DISABLE_OVERLAY_FLAG", &format!("{}u", assets::EntityRenderMaterialState::DISABLE_OVERLAY))
        .replace("ACTOR_ALPHA_TEST_THRESHOLD", &format!("{:?}", assets::ENTITY_ALPHA_TEST_THRESHOLD))
        .replace("MODEL_RANDOM_OFFSET_FLAG", &format!("{}u", meshing::MODEL_REF_FLAG_RANDOM_OFFSET))
        .replace("MODEL_BAMBOO_FLAG", &format!("{}u", assets::MODEL_TEMPLATE_FLAG_BAMBOO))
        .replace("BAMBOO_STEM_SIDE_QUAD_MASK", &format!("{}u", assets::bamboo::STEM_SIDE_QUAD_MASK))
        .replace("BAMBOO_POSITIVE_X_LEAF_QUAD", &format!("{}u", assets::bamboo::POSITIVE_X_LEAF_QUAD))
        .replace("BAMBOO_POSITIVE_Z_LEAF_QUAD", &format!("{}u", assets::bamboo::POSITIVE_Z_LEAF_QUAD))
        .replace("// BAMBOO_CONSTANTS", &format!(
            "const BAMBOO_OFFSET_MIN: f32 = {:?};\nconst BAMBOO_OFFSET_STEP: f32 = {:?};\nconst BAMBOO_STEM_UV_STRIDE: f32 = {:?};\nconst BAMBOO_LEAF_PLANE_INSET: f32 = {:?};",
            block_transform::bamboo::OFFSET_MIN,
            block_transform::bamboo::OFFSET_STEP,
            assets::bamboo::STEM_UV_STRIDE,
            assets::bamboo::LEAF_PLANE_INSET,
        ))
        .replace("MODEL_LILY_PAD_FLAG", &format!("{}u", assets::MODEL_TEMPLATE_FLAG_LILY_PAD))
        .replace("MATERIAL_DISABLE_AO_FLAG", &format!("{}u", assets::MATERIAL_FLAG_DISABLE_AO))
        .replace("MATERIAL_DISABLE_FACE_DIMMING_FLAG", &format!("{}u", assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING))
        .replace("MATERIAL_CAULDRON_WATER_FLAG", &format!("{}u", assets::MATERIAL_FLAG_CAULDRON_WATER))
        .replace("CAULDRON_WATER_COLOUR", &format!("vec4<f32>({}) / 255.0", assets::CAULDRON_WATER_COLOUR.map(|c| format!("{c}.0")).join(", ")))
        .replace("// ANIMATION_GPU_LAYOUT", "struct AnimationGpu { frame_start: u32, frame_count: u32, ticks_per_frame: u32, flags: u32 }")
        .replace("// LIQUID_GEOMETRY_CONSTANTS", &format!(
            "const LIQUID_FACE_INSET: f32 = {:?};\nconst LIQUID_TOP_INSET_BIT: u32 = {}u;\nconst LIQUID_DEPTH_WRITE_BIT: u32 = {}u;\nconst LIQUID_TWO_SIDED_BIT: u32 = {}u;\nconst TRANSPARENT_WATER_DRAW_FLAG: u32 = {}u;\nconst TRANSPARENT_WATER_RECORD_FLAG: u32 = {}u;",
            meshing::liquid::LIQUID_FACE_INSET,
            meshing::liquid::LIQUID_TOP_INSET_BIT,
            meshing::liquid::LIQUID_DEPTH_WRITE_BIT,
            meshing::liquid::LIQUID_TWO_SIDED_BIT,
            meshing::liquid::TRANSPARENT_WATER_DRAW_FLAG,
            meshing::liquid::TRANSPARENT_WATER_RECORD_FLAG,
        ))
        .replace(
            "// ACTOR_SHADE_CONSTANTS",
            &format!(
                "const ACTOR_SHADE: array<f32, 5> = array({});",
                render_api::ACTOR_SHADE_COEFFICIENTS
                    .map(|coefficient| format!("{coefficient:?}"))
                    .join(", "),
            ),
        )
        .replace(
            "MATERIAL_TWO_SIDED_FLAG",
            &format!("{}u", assets::MATERIAL_FLAG_TWO_SIDED),
        )
        .replace(
            "MATERIAL_NATIVE_LEAF_COLOUR_FLAG",
            &format!("{}u", assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR),
        )
        .replace(
            "MATERIAL_OVERLAY_MASK_FLAG",
            &format!("{}u", assets::MATERIAL_FLAG_OVERLAY_MASK),
        )
        .replace(
            "MATERIAL_ALPHA_CUTOUT_FLAG",
            &format!("{}u", assets::MATERIAL_FLAG_ALPHA_CUTOUT),
        )
        .replace(
            "MATERIAL_ISOTROPIC_FLAG",
            &format!("{}u", assets::MATERIAL_FLAG_ISOTROPIC),
        )
        .replace(
            "MATERIAL_LEAF_AO_EXPONENT_MASK",
            &format!("{}u", assets::MATERIAL_LEAF_AO_EXPONENT_MASK),
        )
        .replace(
            "MATERIAL_LEAF_AO_EXPONENT_SHIFT",
            &format!("{}u", assets::MATERIAL_LEAF_AO_EXPONENT_SHIFT),
        )
        .replace(
            "MATERIAL_LEAF_AO_EXPONENT_SCALE",
            &format!("{}.0", assets::MATERIAL_LEAF_AO_EXPONENT_SCALE),
        )
        .replace(
            "NATIVE_LEAF_TEXTURE_BINDING_0",
            &NATIVE_LEAF_TEXTURE_BINDINGS[0].to_string(),
        )
        .replace(
            "NATIVE_LEAF_TEXTURE_BINDING_1",
            &NATIVE_LEAF_TEXTURE_BINDINGS[1].to_string(),
        )
        .replace(
            "NATIVE_LEAF_SAMPLER_BINDING",
            &NATIVE_LEAF_SAMPLER_BINDING.to_string(),
        )
        .replace(
            "PBR_NORMAL_TEXTURE_BINDING_0",
            &PBR_NORMAL_TEXTURE_BINDINGS[0].to_string(),
        )
        .replace(
            "PBR_NORMAL_TEXTURE_BINDING_1",
            &PBR_NORMAL_TEXTURE_BINDINGS[1].to_string(),
        )
        .replace(
            "PBR_MER_TEXTURE_BINDING_0",
            &PBR_MER_TEXTURE_BINDINGS[0].to_string(),
        )
        .replace(
            "PBR_MER_TEXTURE_BINDING_1",
            &PBR_MER_TEXTURE_BINDINGS[1].to_string(),
        )
        .replace("PBR_SAMPLER_BINDING", &PBR_SAMPLER_BINDING.to_string())
        .replace(
            "ENHANCED_COLOR_TEXTURE_BINDING_0",
            &ENHANCED_COLOR_TEXTURE_BINDINGS[0].to_string(),
        )
        .replace(
            "ENHANCED_COLOR_TEXTURE_BINDING_1",
            &ENHANCED_COLOR_TEXTURE_BINDINGS[1].to_string(),
        )
        .replace(
            "ENHANCED_NORMAL_TEXTURE_BINDING_0",
            &ENHANCED_NORMAL_TEXTURE_BINDINGS[0].to_string(),
        )
        .replace(
            "ENHANCED_NORMAL_TEXTURE_BINDING_1",
            &ENHANCED_NORMAL_TEXTURE_BINDINGS[1].to_string(),
        )
        .replace(
            "ENHANCED_MER_TEXTURE_BINDING_0",
            &ENHANCED_MER_TEXTURE_BINDINGS[0].to_string(),
        )
        .replace(
            "ENHANCED_MER_TEXTURE_BINDING_1",
            &ENHANCED_MER_TEXTURE_BINDINGS[1].to_string(),
        )
        .replace("ENHANCED_SAMPLER_BINDING", &ENHANCED_SAMPLER_BINDING.to_string())
        .replace(
            "ENHANCED_TEXTURE_REF_BINDING",
            &ENHANCED_TEXTURE_REF_BINDING.to_string(),
        )
}
