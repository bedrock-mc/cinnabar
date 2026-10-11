//! The camera overlay samples the equipped item's flat icon, independently of held models.

use super::*;
use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex};

impl UiPresentationRuntime {
    /// Paints the event's retained icon through the complete activation transform.
    pub(super) fn append_item_activation(
        &self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        viewport: [f32; 2],
        origin: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let Some(activation) = runtime.item_activation.as_ref() else {
            return Ok(());
        };
        let Some(progress) = activation.progress() else {
            return Ok(());
        };
        let Some(icon) = self.item_icon(&activation.identifier, activation.metadata) else {
            return Ok(());
        };
        if viewport.iter().any(|side| *side <= 0.0) {
            return Ok(());
        }
        let mesh = activation_mesh(icon, progress, activation.offsets, viewport)
            .map_err(|_| UiPresentationError::Tree(ui::UiError::DrawIndexOverflow))?;
        nodes.push(
            UiNode::new(
                UiNodeId::new(*next),
                None,
                rect(
                    origin[0],
                    origin[1],
                    origin[0] + viewport[0],
                    origin[1] + viewport[1],
                )?,
            )
            .with_viewport_clip(rect(0.0, 0.0, viewport[0], viewport[1])?)
            .with_visual(UiVisual::Mesh(Arc::new(mesh))),
        );
        *next += 1;
        Ok(())
    }
}

/// Projects the rotating icon quad through the camera overlay's orthographic frame.
fn activation_mesh(
    icon: IconRef,
    t: f32,
    offsets: [i32; 2],
    viewport: [f32; 2],
) -> Result<UiMesh, ui::UiMeshError> {
    let t2 = t * t;
    let phase = (4.0 * t - 13.8 * t2 + 25.5 * t * t2 - 24.95 * t2 * t2 + 10.25 * t * t2 * t2)
        * std::f32::consts::PI;
    let pulse = lookup_sine(phase);
    let scale = 50.0 + 175.0 * pulse;
    let wobble = lookup_sine(t * 8.0 + std::f32::consts::FRAC_PI_2) * 6.0_f32.to_radians();
    let (sin_wobble, cos_wobble) = wobble.sin_cos();
    let (sin_turn, cos_turn) = (pulse.abs() * 720.0_f32.to_radians()).sin_cos();
    let movement = lookup_sine(phase * 2.0).abs();
    let translate = std::array::from_fn::<_, 2, _>(|axis| {
        viewport[axis] * 0.25 * (1.0 + offsets[axis] as f32 * movement)
    });
    let corners = [[0.5, 0.75], [0.5, -0.25], [-0.5, -0.25], [-0.5, 0.75]];
    let uvs = [[0, 1], [0, 3], [2, 3], [2, 1]];
    let vertices: Vec<_> = corners
        .into_iter()
        .zip(uvs)
        .map(|([x, y], [u, v])| {
            // Matrix order is translation, scale, Y turn, X tilt, Z tilt.
            let zx = x * cos_wobble - y * sin_wobble;
            let zy = x * sin_wobble + y * cos_wobble;
            let rx = zx * cos_turn + zy * sin_wobble * sin_turn;
            let ry = zy * cos_wobble;
            UiMeshVertex {
                position: [
                    2.0 * (translate[0] + scale * rx) / viewport[0],
                    (translate[1] - scale * ry + viewport[1] * 0.15) / (viewport[1] * 0.65),
                ],
                clip_z: 0.0,
                clip_w: 1.0,
                uv: [f32::from(icon.uv[u]), f32::from(icon.uv[v])],
                color: [255; 4],
                model_light: 1.0,
                overlay_color: [0.0; 4],
                style_flags: 0,
                alpha_test: false,
            }
        })
        .collect();
    UiMesh::new(
        vertices.into(),
        Arc::from([0, 1, 2, 0, 2, 3]),
        Arc::from([UiMeshBatch {
            texture_page: icon.page,
            index_range: 0..6,
            blend: UiBlendMode::Alpha,
            depth_test: false,
            depth_write: false,
            alpha_cutoff: None,
        }]),
    )
}

/// The camera animation uses a truncated index into a full-turn sine table.
fn lookup_sine(angle: f32) -> f32 {
    let index = (angle * 10_430.378) as i32 & 0xffff;
    (index as f32 * (std::f32::consts::TAU / 65_536.0)).sin()
}

#[cfg(test)]
mod tests;
