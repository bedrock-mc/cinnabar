//! Evaluated shading shared by held and worn attachable submissions.

use actor_animation::RenderTextureLayer;
use render_model::ActorRigSubmission;

/// Copies only draw style, leaving the evaluated layer's pose and artwork borrowed.
#[derive(Clone, Copy)]
pub(super) struct LayerStyle {
    material: assets::EntityRenderMaterial,
    material_state: Option<assets::EntityRenderMaterialState>,
    color: [f32; 4],
    overlay: [f32; 4],
    uv_anim: [f32; 4],
    ignore_lighting: bool,
    light_color_multiplier: f32,
}

impl LayerStyle {
    /// Retains the selected controller layer's shading without retaining its pose.
    pub(super) fn from_layer(layer: &RenderTextureLayer) -> Self {
        Self {
            material: layer.material,
            material_state: layer.material_state,
            color: layer.color,
            overlay: layer.overlay,
            uv_anim: layer.uv_anim,
            ignore_lighting: layer.ignore_lighting,
            light_color_multiplier: layer.light_color_multiplier,
        }
    }

    /// Applies authored style, preserving stack glint, dye fallback and inherited world light.
    pub(super) fn apply(self, submission: &mut ActorRigSubmission) {
        submission.material.kind = self.material;
        submission.material.state = self.material_state;
        submission.material.dissolve_multiplier = self.overlay[3];
        submission.material.light_color_multiplier = self.light_color_multiplier;
        if self.color != [1.0; 4] {
            submission.tint = render_model::pack_overlay_rgba8(self.color);
        }
        // Dissolve alpha controls coverage, not the vertex shader's hurt lighting.
        submission.overlay_rgba8 = if matches!(
            self.material,
            assets::EntityRenderMaterial::DissolveDepth
                | assets::EntityRenderMaterial::DissolveColor
        ) {
            0
        } else {
            render_model::pack_overlay_rgba8(self.overlay)
        };
        submission.uv_anim = self.uv_anim;
        if self.ignore_lighting {
            submission.light = render_model::pack_actor_light_without_lightmap();
        }
    }
}
