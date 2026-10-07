use bevy::prelude::Shader;
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct UiViewportUniform {
    pub(super) viewport_size: [f32; 2],
    /// Seconds since the UI renderer started; animates the item glint.
    pub(super) time_seconds: f32,
    pub(super) glint_strength: f32,
}

pub(crate) fn source(raw: &str) -> String {
    // Keep the GPU style bits owned by the renderer, with no UI-crate dependency or WGSL copy.
    raw.replace(
        "UI_STYLE_ALPHA_TEST",
        &format!("{}u", render_model::UI_STYLE_ALPHA_TEST),
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
        "FONT_STYLE_COVERAGE_GAMMA",
        &format!("{}u", assets::FONT_STYLE_COVERAGE_GAMMA),
    )
    .replace("FONT_STYLE_SDF", &format!("{}u", assets::FONT_STYLE_SDF))
}

pub(super) fn from_wgsl(raw: &str, path: impl Into<String>) -> Shader {
    crate::shader_safety::from_wgsl(source(raw), path)
}
