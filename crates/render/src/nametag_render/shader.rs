/// Labels use straight-alpha over-compositing for both colour and scene alpha.
pub(crate) const NAMETAG_BLEND: wgpu::BlendState = wgpu::BlendState::ALPHA_BLENDING;

use bevy::prelude::Shader;
use render_model::{NAMETAG_ALPHA_CUTOFF, NAMETAG_BLOCKS_PER_FONT_PIXEL};

/// Substitutes the shared label scale and alpha cutoff before registering the checked WESL module.
pub(super) fn from_wesl(raw: &str, path: impl Into<String>) -> Shader {
    let source = raw
        .replace(
            "NAMETAG_SCALE_VALUE",
            &NAMETAG_BLOCKS_PER_FONT_PIXEL.to_string(),
        )
        .replace(
            "NAMETAG_ALPHA_CUTOFF_VALUE",
            &NAMETAG_ALPHA_CUTOFF.to_string(),
        );
    crate::shader_safety::from_wesl(source, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_aligned_shader_validates_with_every_material_variant() {
        let shader = from_wesl(include_str!("../nametag.wesl"), "nametag test");
        for definitions in [&[][..], &["NAMETAG_GAMMA_BLEND"][..]] {
            crate::shader_source::composed(shader.source.as_str(), definitions);
        }
    }

    #[test]
    fn label_coverage_uses_the_java_cutoff() {
        let shader = from_wesl(include_str!("../nametag.wesl"), "nametag test");
        assert_eq!(
            crate::shader_source::alpha_discard_threshold(
                shader.source.as_str(),
                "nametag_fragment"
            ),
            Some(NAMETAG_ALPHA_CUTOFF)
        );
    }
}
