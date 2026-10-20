use bevy::prelude::Shader;
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct UiViewportUniform {
    pub(super) viewport_size: [f32; 2],
    /// Seconds since the UI renderer started; animates the item glint.
    pub(super) time_seconds: f32,
    pub(super) glint_strength: f32,
}

pub(crate) fn source(raw: &str) -> String {
    // Keep the GPU style bits owned by the renderer, with no UI-crate dependency or WGSL copy.
    raw.replace(
        "GLINT_TINT_RGB",
        &crate::material_shader::glint_tint_source(),
    )
    .replace(
        "UI_STYLE_ALPHA_TEST",
        &format!("{}u", render_model::UI_STYLE_ALPHA_TEST),
    )
    .replace(
        "UI_STYLE_COVERAGE_PAINT",
        &format!("{}u", render_api::UI_STYLE_COVERAGE_PAINT),
    )
    .replace(
        "UI_STYLE_GLINT",
        &format!("{}u", render_model::UI_STYLE_GLINT),
    )
    .replace(
        "UI_STYLE_COLOR_MASK",
        &format!("{}u", render_model::UI_STYLE_COLOR_MASK),
    )
    .replace(
        "UI_STYLE_ADDITIVE",
        &format!("{}u", render_model::UI_STYLE_ADDITIVE),
    )
    .replace(
        "UI_STYLE_PARTICLE",
        &format!("{}u", render_model::UI_STYLE_PARTICLE),
    )
    .replace(
        "UI_STYLE_RADIAL_GRADIENT",
        &format!("{}u", render_model::UI_STYLE_RADIAL_GRADIENT),
    )
    .replace(
        "FONT_STYLE_COVERAGE_GAMMA",
        &format!("{}u", assets::FONT_STYLE_COVERAGE_GAMMA),
    )
    .replace("FONT_STYLE_SDF", &format!("{}u", assets::FONT_STYLE_SDF))
    .replace(
        "SDF_DISTANCE_SCALE",
        &format!("{:?}", render_model::font_shading::SDF_DISTANCE_SCALE),
    )
    .replace(
        "SDF_DISTANCE_BIAS",
        &format!("{:?}", render_model::font_shading::SDF_DISTANCE_BIAS),
    )
    .replace(
        "SDF_SMOOTHING_SCALE",
        &format!("{:?}", render_model::font_shading::SDF_SMOOTHING_SCALE),
    )
    .replace(
        "SDF_MIN_THRESHOLD",
        &format!("{:?}", render_model::font_shading::SDF_MIN_THRESHOLD),
    )
    .replace(
        "COVERAGE_GAMMA_BASE",
        &format!("{:?}", render_model::font_shading::COVERAGE_GAMMA_BASE),
    )
    .replace("COVERAGE_LUMINANCE", &{
        let [r, g, b] = render_model::font_shading::COVERAGE_LUMINANCE;
        format!("vec3<f32>({r:?}, {g:?}, {b:?})")
    })
}

pub(super) fn from_wgsl(raw: &str, path: impl Into<String>) -> Shader {
    crate::shader_safety::from_wgsl(source(raw), path)
}
