//! Vanilla actor flame geometry projected in the UI actor frame.

use ui::{UiBlendMode, UiMeshBatch, UiMeshVertex};

use super::super::{
    IconRef, PREVIEW_FEET_Y, PREVIEW_HEIGHT, PREVIEW_PIXELS_PER_BLOCK, PREVIEW_WIDTH,
};

#[derive(Clone, Copy)]
pub struct PreviewFire {
    pub texture: IconRef,
    /// Authoritative collision-box width and height, independent of the player model scale.
    pub size: [f32; 2],
    /// Outer HUD translation in block units, separate from actor/model scale.
    pub outer_y: f32,
}

pub(super) fn append(
    vertices: &mut Vec<UiMeshVertex>,
    batches: &mut Vec<UiMeshBatch>,
    fire: PreviewFire,
) -> Option<()> {
    let [width, height] = fire.size;
    if !fire.outer_y.is_finite()
        || [width, height]
            .iter()
            .any(|axis| !axis.is_finite() || *axis <= 0.0)
    {
        return None;
    }
    let horizontal = width * 1.4;
    let vertical = width.min(height) * 1.4;
    // UI rendering skips the world camera billboard and reverses the native depth offset.
    let depth = -((height / horizontal).floor() * 0.02 - 0.3);
    let [left, top, right, bottom] = fire.texture.uv.map(f32::from);
    if left >= right || top >= bottom || right - left != bottom - top {
        return None;
    }
    let start = u32::try_from(vertices.len()).ok()?;
    // The lower and upper pair each have independently reversed faces. Native's
    // upper faces are separated by +/-0.03 before collision-box scaling.
    for (half, low, high, z, reverse) in [
        (0.5, 0.0, 1.4, 0.0, false),
        (0.45, 0.45, 1.85, 0.03, false),
        (0.5, 0.0, 1.4, 0.0, true),
        (0.45, 0.45, 1.85, -0.03, true),
    ] {
        let positions = [
            [half * horizontal, low * vertical],
            [-half * horizontal, low * vertical],
            [-half * horizontal, high * vertical],
            [half * horizontal, high * vertical],
        ];
        // Preserve the native 0.05-texel V inset, without quantizing UV edges.
        let uv = [
            [right, bottom - 0.05],
            [left, bottom - 0.05],
            [left, top + 0.05],
            [right, top + 0.05],
        ];
        let corners = if reverse {
            [3, 2, 1, 3, 1, 0]
        } else {
            [0, 1, 2, 0, 2, 3]
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
