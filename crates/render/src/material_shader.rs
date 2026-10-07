//! The material carrier and WGSL consume the same flag discriminants.

pub(crate) const NATIVE_LEAF_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [16, 17];
pub(crate) const NATIVE_LEAF_SAMPLER_BINDING: u32 = 18;
pub(crate) const PBR_NORMAL_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [19, 20];
pub(crate) const PBR_MER_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [21, 22];
pub(crate) const PBR_SAMPLER_BINDING: u32 = 23;
pub(crate) const ENHANCED_COLOR_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [24, 25];
pub(crate) const ENHANCED_NORMAL_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [26, 27];
pub(crate) const ENHANCED_MER_TEXTURE_BINDINGS: [u32; assets::MAX_TEXTURE_PAGES] = [28, 29];
pub(crate) const ENHANCED_SAMPLER_BINDING: u32 = 30;
pub(crate) const ENHANCED_TEXTURE_REF_BINDING: u32 = 31;
pub(crate) const CHUNK_SAMPLER_COUNT: u32 = 4;
pub(crate) const CHUNK_SAMPLED_TEXTURE_BINDINGS: u32 = (assets::MAX_TEXTURE_PAGES
    + NATIVE_LEAF_TEXTURE_BINDINGS.len()
    + PBR_NORMAL_TEXTURE_BINDINGS.len()
    + PBR_MER_TEXTURE_BINDINGS.len()
    + ENHANCED_COLOR_TEXTURE_BINDINGS.len()
    + ENHANCED_NORMAL_TEXTURE_BINDINGS.len()
    + ENHANCED_MER_TEXTURE_BINDINGS.len())
    as u32;

pub(crate) fn chunk_atlas_views_fit(limits: &wgpu::Limits) -> bool {
    limits.max_sampled_textures_per_shader_stage >= CHUNK_SAMPLED_TEXTURE_BINDINGS
        && limits.max_samplers_per_shader_stage >= CHUNK_SAMPLER_COUNT
        && limits.max_bindings_per_bind_group > ENHANCED_TEXTURE_REF_BINDING
}

/// Current terrain atlas binding: Dragon 0x155 -> BGFX 0x16a.
/// Sampler conversion and D3D creation establish point
/// min/mag, linear mip, and clamp UVW. Keep non-leaf materials unchanged.
pub(crate) fn native_leaf_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("native terrain leaf sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        min_filter: wgpu::FilterMode::Nearest,
        mag_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    }
}

pub(crate) fn pbr_sampler_descriptor() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("enhanced linear PBR sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    }
}

pub(crate) fn source(source: &str) -> String {
    source
        .replace("// ENHANCED_PBR_SAMPLING", include_str!("enhanced/pbr_sampling.wgsl"))
        .replace("// ENHANCED_PBR_CONSTANTS", &format!(
            "const PBR_REF_COLOR:u32={}u;\nconst PBR_REF_NORMAL:u32={}u;\nconst PBR_REF_HEIGHT:u32={}u;\nconst PBR_REF_MATERIAL:u32={}u;\nconst PBR_REF_LABPBR:u32={}u;\nconst PBR_REF_OCCLUSION:u32={}u;\nconst PBR_REF_SUBSURFACE:u32={}u;\nconst PBR_HEIGHT_SCALE:f32={:?};",
            assets::PBR_REF_COLOR, assets::PBR_REF_NORMAL, assets::PBR_REF_HEIGHT, assets::PBR_REF_MATERIAL,
            assets::PBR_REF_LABPBR, assets::PBR_REF_OCCLUSION, assets::PBR_REF_SUBSURFACE, assets::PBR_HEIGHT_SCALE))
        .replace("MODEL_LILY_PAD_FLAG", &format!("{}u", assets::MODEL_TEMPLATE_FLAG_LILY_PAD))
        .replace("// ANIMATION_GPU_LAYOUT", "struct AnimationGpu { frame_start: u32, frame_count: u32, ticks_per_frame: u32, flags: u32, uv_scale: f32 }")
        .replace("// LIQUID_GEOMETRY_CONSTANTS", &format!(
            "const LIQUID_FACE_INSET: f32 = {:?};\nconst LIQUID_TOP_INSET_BIT: u32 = {}u;\nconst LIQUID_DEPTH_WRITE_BIT: u32 = {}u;\nconst LIQUID_TWO_SIDED_BIT: u32 = {}u;",
            meshing::liquid::LIQUID_FACE_INSET,
            meshing::liquid::LIQUID_TOP_INSET_BIT,
            meshing::liquid::LIQUID_DEPTH_WRITE_BIT,
            meshing::liquid::LIQUID_TWO_SIDED_BIT,
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
            "MATERIAL_LEAF_ISOTROPIC_FLAG",
            &format!("{}u", assets::MATERIAL_FLAG_LEAF_ISOTROPIC),
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

/// Loads generated material contracts with the shader source.
pub(crate) fn shader(source: impl Into<String>, path: impl Into<String>) -> bevy::shader::Shader {
    crate::shader_safety::from_wgsl(self::source(&source.into()), path)
}
