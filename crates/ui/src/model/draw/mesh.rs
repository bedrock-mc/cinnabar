use super::{DrawSpace, UiDrawBatch, UiError, UiVertex};
use crate::{UiLimits, UiRect};

/// One source vertex's emitted index within the current material.
#[derive(Clone)]
struct VertexMapping {
    index: u32,
    #[cfg(test)]
    _copies: tests::MappingCopies,
}

impl VertexMapping {
    const EMPTY: Self = Self {
        index: u32::MAX,
        #[cfg(test)]
        _copies: tests::MappingCopies,
    };
}

/// Emits shared indexed vertices once per material while preserving topology and cutoff boundaries.
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
    // Reuse vertices inside each material while keeping different material cutoffs independent.
    let mut quad_remap = [const { VertexMapping::EMPTY }; 4];
    let mut large_remap;
    let remap = if mesh.vertices().len() <= quad_remap.len() {
        &mut quad_remap[..mesh.vertices().len()]
    } else {
        large_remap = vec![VertexMapping::EMPTY; mesh.vertices().len()];
        &mut large_remap[..]
    };
    let depth_scope = mesh
        .batches()
        .iter()
        .any(|material| material.depth_test || material.depth_write)
        .then_some(space.node.get());
    for material in mesh.batches() {
        // Previous materials' mappings precede this vertex range, so no map clearing is needed.
        let first_vertex = u32::try_from(vertices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
        let start = u32::try_from(indices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
        for &index in
            &mesh.indices()[material.index_range.start as usize..material.index_range.end as usize]
        {
            if (first_vertex..u32::MAX).contains(&remap[index as usize].index) {
                indices.push(remap[index as usize].index);
                continue;
            }
            let vertex = mesh.vertices()[index as usize];
            if vertices.len() == UiLimits::MAX_UI_VERTICES {
                return Err(UiError::VertexLimitExceeded {
                    actual: vertices.len() + 1,
                    limit: UiLimits::MAX_UI_VERTICES,
                });
            }
            let mapped = u32::try_from(vertices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
            remap[index as usize].index = mapped;
            indices.push(mapped);
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
                style_flags: u32::from(vertex.style_flags),
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
            isolated_depth_scope: depth_scope,
            filter_group: space.filter_for(material.blend),
            index_range: start
                ..u32::try_from(indices.len()).map_err(|_| UiError::DrawIndexOverflow)?,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, sync::Arc};

    use crate::{
        SafeArea, UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex, UiNode, UiNodeId, UiPoint,
        UiScale, UiTree, UiVisual,
    };

    thread_local! {
        static MAPPING_COPIES: Cell<usize> = const { Cell::new(0) };
    }

    /// Counts remap initialization and clearing independently of elapsed time.
    pub(super) struct MappingCopies;

    impl Clone for MappingCopies {
        fn clone(&self) -> Self {
            MAPPING_COPIES.update(|count| count + 1);
            Self
        }
    }

    /// Sparse materials touch only their own source vertices while retaining shared topology.
    #[test]
    fn sparse_materials_do_not_clear_the_full_vertex_map() {
        let vertex_count = 100_000;
        let material_count = 128;
        let source = UiMeshVertex {
            position: [0.25, 0.5],
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [0.0; 2],
            color: [255; 4],
            model_light: 1.0,
            overlay_color: [0.0; 4],
            style_flags: 0,
            alpha_test: false,
        };
        let mesh = Arc::new(
            UiMesh::new(
                vec![source; vertex_count].into(),
                (0..material_count)
                    .flat_map(|i| [i, i + 1, i, i, i + 1, i])
                    .collect::<Vec<_>>()
                    .into(),
                (0..material_count)
                    .map(|i| UiMeshBatch {
                        texture_page: 0,
                        index_range: i * 6..(i + 1) * 6,
                        blend: UiBlendMode::Alpha,
                        depth_test: false,
                        depth_write: false,
                        alpha_cutoff: Some(i as f32 / material_count as f32),
                    })
                    .collect::<Vec<_>>()
                    .into(),
            )
            .unwrap(),
        );
        let bounds = super::UiRect::new(
            UiPoint::new(0.0, 0.0).unwrap(),
            UiPoint::new(10.0, 10.0).unwrap(),
        )
        .unwrap();
        let mut tree = UiTree::new(vec![
            UiNode::new(UiNodeId::new(1), None, bounds).with_visual(UiVisual::Mesh(mesh)),
        ])
        .unwrap();
        tree.layout(bounds, UiScale::default(), SafeArea::ZERO)
            .unwrap();
        MAPPING_COPIES.set(0);
        let draw = tree.build_draw_list().unwrap();
        assert!(
            MAPPING_COPIES.get() <= vertex_count,
            "remap clearing copied {} entries for {vertex_count} source vertices",
            MAPPING_COPIES.get()
        );
        assert_eq!(draw.vertices.len(), material_count as usize * 2);
        for (i, batch) in draw.batches.iter().enumerate() {
            let first = i as u32 * 2;
            assert_eq!(
                &draw.indices[batch.index_range.start as usize..batch.index_range.end as usize],
                &[first, first + 1, first, first, first + 1, first]
            );
            assert_eq!(
                draw.vertices[first as usize].alpha_cutoff,
                i as f32 / material_count as f32
            );
        }
    }
}
