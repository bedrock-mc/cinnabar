//! The material carrier and WGSL consume the same flag discriminants.

pub(crate) const NATIVE_LEAF_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [16, 17];
pub(crate) const NATIVE_LEAF_SAMPLER_BINDING: u32 = 18;
pub(crate) const BIOME_QUERY_TABLES_BINDING: u32 = 19;
pub(crate) const CHUNK_SAMPLER_COUNT: u32 = 2;
pub(crate) const CHUNK_SAMPLED_TEXTURE_BINDINGS: u32 =
    (assets::MAX_TEXTURE_PAGES + NATIVE_LEAF_TEXTURE_BINDINGS.len()) as u32;

const TEXTURE_WIDTH_SHIFT: u32 = 11;
const TEXTURE_HEIGHT_SHIFT: u32 = 21;
const TEXTURE_DIMENSION_MASK: u32 = 1023;
const TEXTURE_GRID_MARKER: u32 = 1 << 9;
const TEXTURE_GRID_SHIFT: u32 = 4;
const TEXTURE_SIZE_EXPONENT_MASK: u32 = 15;

/// Encodes admitted image dimensions in GPU-only reference bits, preserving page and layer.
pub(crate) fn gpu_texture_ref(
    reference: assets::TextureRef,
    dimensions: [u32; 2],
    page_size: u32,
) -> u32 {
    if dimensions == [page_size; 2] {
        reference.raw()
    } else {
        reference.raw()
            | (dimensions[0] << TEXTURE_WIDTH_SHIFT)
            | (dimensions[1] << TEXTURE_HEIGHT_SHIFT)
    }
}

/// Encodes the terrain entry's UV grid together with its source pixel dimensions.
pub(crate) fn gpu_grid_texture_ref(
    reference: assets::TextureRef,
    dimensions: [u32; 2],
    page_size: u32,
    grid: u8,
) -> u32 {
    if grid == 0 {
        return gpu_texture_ref(reference, dimensions, page_size);
    }
    let width =
        TEXTURE_GRID_MARKER | (u32::from(grid) << TEXTURE_GRID_SHIFT) | dimensions[0].ilog2();
    reference.raw() | (width << TEXTURE_WIDTH_SHIFT) | (dimensions[1] << TEXTURE_HEIGHT_SHIFT)
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
        && limits.max_bindings_per_bind_group > BIOME_QUERY_TABLES_BINDING
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
        mipmap_filter: wgpu::FilterMode::Linear,
        lod_max_clamp: (assets::VANILLA_TERRAIN_MIP_COUNT - 1) as f32,
        ..Default::default()
    }
}

pub(crate) fn source(source: &str) -> String {
    source
        .replace("ACTOR_LIGHT_WORLD_FLAG", &format!("{}u", render_api::ACTOR_LIGHT_WORLD))
        .replace("ACTOR_LIGHT_DIRECTIONAL_FLAG", &format!("{}u", render_api::ACTOR_LIGHT_DIRECTIONAL))
        .replace("GPU_TEXTURE_GRID_MARKER", &format!("{TEXTURE_GRID_MARKER}u"))
        .replace("GPU_TEXTURE_GRID_SHIFT", &format!("{TEXTURE_GRID_SHIFT}u"))
        .replace("GPU_TEXTURE_SIZE_EXPONENT_MASK", &format!("{TEXTURE_SIZE_EXPONENT_MASK}u"))
        .replace("TERRAIN_QUAD_SHIFT_MASK", &format!("{}u", assets::TERRAIN_QUAD_SHIFT_MASK))
        .replace("GPU_TEXTURE_WIDTH_SHIFT", &format!("{TEXTURE_WIDTH_SHIFT}u"))
        .replace("GPU_TEXTURE_HEIGHT_SHIFT", &format!("{TEXTURE_HEIGHT_SHIFT}u"))
        .replace("GPU_TEXTURE_DIMENSION_MASK", &format!("{TEXTURE_DIMENSION_MASK}u"))
        .replace("BLOCK_OVERLAY_FACE_OFFSET", &format!("{:?}", render_api::BLOCK_OVERLAY_FACE_OFFSET))
        .replace("ACTOR_MATERIAL_GLINT", &format!("{}u", assets::EntityRenderMaterial::Glint as u32))
        .replace("ACTOR_MATERIAL_DEFAULT", &format!("{}u", assets::EntityRenderMaterial::Default as u32))
        .replace("ACTOR_MATERIAL_DRAGON", &format!("{}u", assets::EntityRenderMaterial::Dragon as u32))
        .replace("ACTOR_MATERIAL_DISSOLVE_DEPTH", &format!("{}u", assets::EntityRenderMaterial::DissolveDepth as u32))
        .replace("ACTOR_MATERIAL_DISSOLVE_COLOR", &format!("{}u", assets::EntityRenderMaterial::DissolveColor as u32))
        .replace("ACTOR_MATERIAL_KIND_MASK", &format!("{}u", assets::EntityRenderMaterialState::KIND_MASK))
        .replace("ACTOR_MATERIAL_AUTHORED_FLAG", &format!("{}u", assets::EntityRenderMaterialState::AUTHORED))
        .replace("ACTOR_MATERIAL_ALPHA_TEST_FLAG", &format!("{}u", assets::EntityRenderMaterialState::ALPHA_TEST))
        .replace("ACTOR_MATERIAL_CULL_FLAG", &format!("{}u", assets::EntityRenderMaterialState::CULL))
        .replace("ACTOR_MATERIAL_EMISSIVE_FLAG", &format!("{}u", assets::EntityRenderMaterialState::EMISSIVE))
        .replace("ACTOR_MATERIAL_DISABLE_OVERLAY_FLAG", &format!("{}u", assets::EntityRenderMaterialState::DISABLE_OVERLAY))
        .replace("ACTOR_ALPHA_TEST_THRESHOLD", &format!("{:?}", assets::ENTITY_ALPHA_TEST_THRESHOLD))
        .replace("MODEL_RANDOM_OFFSET_FLAG", &format!("{}u", meshing::MODEL_REF_FLAG_RANDOM_OFFSET))
        .replace("MODEL_BAMBOO_FLAG", &format!("{}u", assets::MODEL_TEMPLATE_FLAG_BAMBOO))
        .replace("BAMBOO_STEM_SIDE_QUAD_MASK", &format!("{}u", meshing::bamboo::STEM_SIDE_QUAD_MASK))
        .replace("BAMBOO_POSITIVE_X_LEAF_QUAD", &format!("{}u", meshing::bamboo::POSITIVE_X_LEAF_QUAD))
        .replace("BAMBOO_POSITIVE_Z_LEAF_QUAD", &format!("{}u", meshing::bamboo::POSITIVE_Z_LEAF_QUAD))
        .replace("// BAMBOO_CONSTANTS", &format!(
            "const BAMBOO_OFFSET_MIN: f32 = {:?};\nconst BAMBOO_OFFSET_STEP: f32 = {:?};\nconst BAMBOO_STEM_UV_STRIDE: f32 = {:?};\nconst BAMBOO_LEAF_PLANE_INSET: f32 = {:?};",
            block_transform::bamboo::OFFSET_MIN,
            block_transform::bamboo::OFFSET_STEP,
            meshing::bamboo::STEM_UV_STRIDE,
            meshing::bamboo::LEAF_PLANE_INSET,
        ))
        .replace("MODEL_LILY_PAD_FLAG", &format!("{}u", assets::MODEL_TEMPLATE_FLAG_LILY_PAD))
        .replace("MATERIAL_DISABLE_AO_FLAG", &format!("{}u", assets::MATERIAL_FLAG_DISABLE_AO))
        .replace("MATERIAL_DISABLE_FACE_DIMMING_FLAG", &format!("{}u", assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING))
        .replace("// ANIMATION_GPU_LAYOUT", "struct AnimationGpu { frame_start: u32, frame_count: u32, ticks_per_frame: u32, flags: u32, uv_scale: f32 }")
        .replace("// LIQUID_GEOMETRY_CONSTANTS", &format!(
            "const LIQUID_FACE_INSET: f32 = {:?};\nconst LIQUID_TOP_INSET_BIT: u32 = {}u;\nconst LIQUID_DEPTH_WRITE_BIT: u32 = {}u;\nconst LIQUID_TWO_SIDED_BIT: u32 = {}u;\nconst TRANSPARENT_WATER_DRAW_FLAG: u32 = {}u;",
            meshing::liquid::LIQUID_FACE_INSET,
            meshing::liquid::LIQUID_TOP_INSET_BIT,
            meshing::liquid::LIQUID_DEPTH_WRITE_BIT,
            meshing::liquid::LIQUID_TWO_SIDED_BIT,
            meshing::liquid::TRANSPARENT_WATER_DRAW_FLAG,
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
}
