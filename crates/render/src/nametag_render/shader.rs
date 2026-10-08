use bevy::prelude::Shader;
use render_model::{
    NAMETAG_ACOS_CUBIC, NAMETAG_ACOS_LINEAR, NAMETAG_BLOCKS_PER_FONT_PIXEL, NAMETAG_HORIZONTAL_ZERO,
};

pub(super) fn from_wgsl(raw: &str, path: impl Into<String>) -> Shader {
    let source = raw
        .replace(
            "NAMETAG_SCALE_VALUE",
            &NAMETAG_BLOCKS_PER_FONT_PIXEL.to_string(),
        )
        .replace(
            "NAMETAG_ACOS_LINEAR_VALUE",
            &NAMETAG_ACOS_LINEAR.to_string(),
        )
        .replace("NAMETAG_ACOS_CUBIC_VALUE", &NAMETAG_ACOS_CUBIC.to_string())
        .replace(
            "NAMETAG_HORIZONTAL_ZERO_VALUE",
            &NAMETAG_HORIZONTAL_ZERO.to_string(),
        );
    crate::shader_safety::from_wgsl(source, path)
}
