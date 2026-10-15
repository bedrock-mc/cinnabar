//! Vanilla actor flame geometry projected in the UI actor frame.

use render_model::actor::flame::{FLAME_FACES, FLAME_SCALE, FLAME_V_INSET_TEXELS, flame_depth};
use ui::{UiBlendMode, UiMeshBatch, UiMeshVertex};

use {
    super::super::{PREVIEW_FEET_Y, PREVIEW_HEIGHT, PREVIEW_PIXELS_PER_BLOCK, PREVIEW_WIDTH},
    ui::IconRef,
};

#[derive(Clone, Copy)]
pub struct PreviewFire {
    pub texture: IconRef,
    /// Authoritative collision-box width and height, independent of the player model scale.
    pub size: [f32; 2],
    /// Outer HUD translation in block units, separate from actor/model scale.
    pub outer_y: f32,
}

const CORNERS: [usize; 6] = [0, 1, 2, 0, 2, 3];

impl PreviewFire {
    /// Counts the authored triangle corners, rejecting invalid flame inputs before projection.
    pub(super) fn vertex_count(self) -> Option<usize> {
        let [width, height] = self.size;
        let [left, top, right, bottom] = self.texture.uv;
        (self.outer_y.is_finite()
            && [width, height]
                .iter()
                .all(|axis| axis.is_finite() && *axis > 0.0)
            && left < right
            && top < bottom
            && right - left == bottom - top)
            .then_some(FLAME_FACES.len() * CORNERS.len())
    }
}

pub(super) fn append(
    vertices: &mut Vec<UiMeshVertex>,
    batches: &mut Vec<UiMeshBatch>,
    fire: PreviewFire,
) -> Option<()> {
    fire.vertex_count()?;
    let [width, height] = fire.size;
    let horizontal = width * FLAME_SCALE;
    let vertical = width.min(height) * FLAME_SCALE;
    // UI rendering skips the world camera billboard and reverses the native depth offset.
    let depth = -flame_depth(width, height);
    let [left, top, right, bottom] = fire.texture.uv.map(f32::from);
    let start = u32::try_from(vertices.len()).ok()?;
    // The lower and upper pair each have independently reversed faces. Native's
    // upper faces are separated by +/-0.03 before collision-box scaling.
    for (half, low, high, z, reverse) in FLAME_FACES {
        let positions = [
            [half * horizontal, low * vertical],
            [-half * horizontal, low * vertical],
            [-half * horizontal, high * vertical],
            [half * horizontal, high * vertical],
        ];
        // Preserve the native 0.05-texel V inset, without quantizing UV edges.
        let uv = [
            [right, bottom - FLAME_V_INSET_TEXELS],
            [left, bottom - FLAME_V_INSET_TEXELS],
            [left, top + FLAME_V_INSET_TEXELS],
            [right, top + FLAME_V_INSET_TEXELS],
        ];
        let corners = if reverse {
            CORNERS.map(|corner| positions.len() - 1 - corner)
        } else {
            CORNERS
        };
        for corner in corners {
            let [x, y] = positions[corner];
            vertices.push(UiMeshVertex {
                position: [
                    (PREVIEW_WIDTH as f32 * 0.5 + x * PREVIEW_PIXELS_PER_BLOCK)
                        / PREVIEW_WIDTH as f32,
                    (PREVIEW_FEET_Y - (y + fire.outer_y) * PREVIEW_PIXELS_PER_BLOCK)
                        / PREVIEW_HEIGHT as f32,
                ],
                clip_z: (z + depth) * horizontal,
                clip_w: 1.0,
                uv: uv[corner],
                color: [255; 4],
                model_light: 1.0,
                overlay_color: [0.0; 4],
                style_flags: 0,
                alpha_test: false,
            });
        }
    }
    batches.push(UiMeshBatch {
        texture_page: fire.texture.page,
        index_range: start..u32::try_from(vertices.len()).ok()?,
        blend: UiBlendMode::Alpha,
        depth_test: true,
        depth_write: true,
        alpha_cutoff: Some(0.5),
    });
    Some(())
}

#[cfg(test)]
mod tests;
