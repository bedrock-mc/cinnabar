use bevy::prelude::Shader;
use render_model::NAMETAG_BLOCKS_PER_FONT_PIXEL;

/// Substitutes the shared font-pixel scale before registering the checked WESL module.
pub(super) fn from_wesl(raw: &str, path: impl Into<String>) -> Shader {
    let source = raw.replace(
        "NAMETAG_SCALE_VALUE",
        &NAMETAG_BLOCKS_PER_FONT_PIXEL.to_string(),
    );
    crate::shader_safety::from_wesl(source, path)
}
