use super::{DrawSpace, UiDrawBatch, UiError, UiVertex};
use crate::{UiLimits, UiRect};

pub(super) fn emit_mesh(
    mesh: &crate::UiMesh,
    bounds: UiRect,
    space: DrawSpace<'_>,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    if super::is_empty(bounds) || mesh.indices().is_empty() {
        return Ok(());
    }
    let next_batches = batches
        .len()
        .checked_add(mesh.batches().len())
        .ok_or(UiError::DrawIndexOverflow)?;
    if next_batches > UiLimits::MAX_DRAW_BATCHES {
        return Err(UiError::DrawBatchLimitExceeded {
            actual: next_batches,
            limit: UiLimits::MAX_DRAW_BATCHES,
        });
    }
    // Material cutoffs belong to batches. Copy each material's referenced vertices
    // separately so sharing a source vertex cannot leak a cutoff into another layer.
    for material in mesh.batches() {
        let start = u32::try_from(indices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
        for &index in
            &mesh.indices()[material.index_range.start as usize..material.index_range.end as usize]
        {
            let vertex = mesh.vertices()[index as usize];
            if vertices.len() == UiLimits::MAX_UI_VERTICES {
                return Err(UiError::VertexLimitExceeded {
                    actual: vertices.len() + 1,
                    limit: UiLimits::MAX_UI_VERTICES,
                });
            }
            indices.push(u32::try_from(vertices.len()).map_err(|_| UiError::DrawIndexOverflow)?);
            let position = [
                bounds.min().x() * vertex.clip_w + vertex.position[0] * bounds.width(),
                bounds.min().y() * vertex.clip_w + vertex.position[1] * bounds.height(),
            ];
            if !position.iter().all(|value| value.is_finite()) {
                return Err(UiError::DrawIndexOverflow);
            }
            vertices.push(UiVertex {
                position,
                clip_z: vertex.clip_z,
                clip_w: vertex.clip_w,
                uv: vertex.uv,
                color: vertex.color,
                style_flags: vertex.style_flags,
                alpha_test: vertex.alpha_test,
                alpha_cutoff: material.alpha_cutoff.unwrap_or(-1.0),
                model_light: vertex.model_light,
                overlay_color: vertex.overlay_color,
            });
        }
        batches.push(UiDrawBatch {
            texture_page: material.texture_page,
            clip: space.clip,
            blend: material.blend,
            depth_test: material.depth_test,
            depth_write: material.depth_write,
            world_projection: false,
            isolated_depth_scope: Some(space.node.get()),
            index_range: start
                ..u32::try_from(indices.len()).map_err(|_| UiError::DrawIndexOverflow)?,
        });
    }
    Ok(())
}
