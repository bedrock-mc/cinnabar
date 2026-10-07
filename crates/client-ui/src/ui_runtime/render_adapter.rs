use std::{fmt, sync::Arc};

use render_model::{
    UiRenderBatch, UiRenderInput, UiRenderRejectReason, UiRenderTextureArray, UiRenderVertex,
    UiScissor,
};
use ui::{DpiScale, SafeArea, UiDrawList, UiLimits, UiRect};

// Style bits pass through verbatim, so the renderer's glint bit must be the UI crate's.
const _: () = assert!(ui::UI_STYLE_GLINT as u32 == render_model::UI_STYLE_GLINT);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiRenderViewport {
    pub physical_size: [u32; 2],
    pub dpi_scale: DpiScale,
    pub safe_area: SafeArea,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiRenderAdapterError {
    VertexLimitExceeded,
    IndexLimitExceeded,
    BatchLimitExceeded,
    InvalidPhysicalViewport,
    CoordinateOverflow,
    InvalidIndexRange { batch: usize },
    RenderInputRejected(UiRenderRejectReason),
}

impl fmt::Display for UiRenderAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "UI render adapter rejected input: {self:?}")
    }
}

impl std::error::Error for UiRenderAdapterError {}

pub fn adapt_ui_draw_list(
    draw_list: &UiDrawList,
    textures: Arc<UiRenderTextureArray>,
    viewport: UiRenderViewport,
) -> Result<UiRenderInput, UiRenderAdapterError> {
    if draw_list.vertices.len() > UiLimits::MAX_UI_VERTICES {
        return Err(UiRenderAdapterError::VertexLimitExceeded);
    }
    if draw_list.indices.len() > UiLimits::MAX_UI_INDICES {
        return Err(UiRenderAdapterError::IndexLimitExceeded);
    }
    if draw_list.batches.len() > UiLimits::MAX_DRAW_BATCHES {
        return Err(UiRenderAdapterError::BatchLimitExceeded);
    }
    if viewport.physical_size.contains(&0) {
        return Err(UiRenderAdapterError::InvalidPhysicalViewport);
    }
    let scale = viewport.dpi_scale.get();
    let vertices = draw_list
        .vertices
        .iter()
        .map(|vertex| {
            let position = [vertex.position[0] * scale, vertex.position[1] * scale];
            if !position.iter().all(|value| value.is_finite()) {
                return Err(UiRenderAdapterError::CoordinateOverflow);
            }
            Ok(UiRenderVertex {
                position,
                clip_z: vertex.clip_z,
                clip_w: vertex.clip_w,
                uv: vertex.uv,
                color: vertex.color,
                style_flags: u32::from(vertex.style_flags)
                    | if vertex.alpha_test {
                        render_model::UI_STYLE_ALPHA_TEST
                    } else {
                        0
                    },
                alpha_cutoff: vertex.alpha_cutoff,
                model_light: vertex.model_light,
                overlay_color: vertex.overlay_color,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut indices = Vec::with_capacity(draw_list.indices.len());
    let mut batches = Vec::with_capacity(draw_list.batches.len());
    for (batch_index, batch) in draw_list.batches.iter().enumerate() {
        let start = usize::try_from(batch.index_range.start)
            .map_err(|_| UiRenderAdapterError::InvalidIndexRange { batch: batch_index })?;
        let end = usize::try_from(batch.index_range.end)
            .map_err(|_| UiRenderAdapterError::InvalidIndexRange { batch: batch_index })?;
        let source_indices = draw_list
            .indices
            .get(start..end)
            .ok_or(UiRenderAdapterError::InvalidIndexRange { batch: batch_index })?;
        let scissor = physical_scissor(batch.clip, scale, viewport.physical_size)?;
        if scissor.width == 0 || scissor.height == 0 {
            continue;
        }
        let first_index = u32::try_from(indices.len())
            .map_err(|_| UiRenderAdapterError::InvalidIndexRange { batch: batch_index })?;
        let index_count = u32::try_from(source_indices.len())
            .map_err(|_| UiRenderAdapterError::InvalidIndexRange { batch: batch_index })?;
        indices.extend_from_slice(source_indices);
        batches.push(
            UiRenderBatch::new(
                u32::from(batch.texture_page),
                scissor,
                first_index,
                index_count,
                match batch.blend {
                    ui::UiBlendMode::Alpha => render_model::UI_BLEND_ALPHA,
                    ui::UiBlendMode::Invert => render_model::UI_BLEND_INVERT,
                },
            )
            .with_depth_test(batch.depth_test)
            .with_depth_write(batch.depth_write)
            .with_world_projection(batch.world_projection)
            .with_isolated_depth_scope(batch.isolated_depth_scope),
        );
    }
    let input = UiRenderInput {
        revision: draw_list.revision,
        viewport_size: viewport.physical_size,
        safe_area: physical_safe_area(viewport.safe_area, scale)?,
        vertices: vertices.into(),
        indices: indices.into(),
        batches: batches.into(),
        textures,
    };
    input
        .validate()
        .map_err(UiRenderAdapterError::RenderInputRejected)?;
    Ok(input)
}

fn physical_scissor(
    clip: UiRect,
    scale: f32,
    viewport: [u32; 2],
) -> Result<UiScissor, UiRenderAdapterError> {
    let left = clip_edge(clip.min().x(), scale, viewport[0], f32::floor)?;
    let top = clip_edge(clip.min().y(), scale, viewport[1], f32::floor)?;
    let right = clip_edge(clip.max().x(), scale, viewport[0], f32::ceil)?;
    let bottom = clip_edge(clip.max().y(), scale, viewport[1], f32::ceil)?;
    Ok(UiScissor::new(
        left,
        top,
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    ))
}

/// Intersects a finite logical clip edge with its physical viewport before conversion.
fn clip_edge(
    value: f32,
    scale: f32,
    limit: u32,
    round: fn(f32) -> f32,
) -> Result<u32, UiRenderAdapterError> {
    let scaled = value * scale;
    if !scaled.is_finite() {
        return Err(UiRenderAdapterError::CoordinateOverflow);
    }
    Ok(round(scaled).clamp(0.0, limit as f32) as u32)
}

fn physical_safe_area(safe_area: SafeArea, scale: f32) -> Result<[u32; 4], UiRenderAdapterError> {
    Ok([
        scaled_ceil(safe_area.left(), scale)?,
        scaled_ceil(safe_area.top(), scale)?,
        scaled_ceil(safe_area.right(), scale)?,
        scaled_ceil(safe_area.bottom(), scale)?,
    ])
}

fn scaled_ceil(value: f32, scale: f32) -> Result<u32, UiRenderAdapterError> {
    scaled_u32(value, scale, f32::ceil)
}

fn scaled_u32(value: f32, scale: f32, round: fn(f32) -> f32) -> Result<u32, UiRenderAdapterError> {
    let scaled = round(value * scale);
    if !scaled.is_finite() || scaled < 0.0 || scaled > u32::MAX as f32 {
        return Err(UiRenderAdapterError::CoordinateOverflow);
    }
    Ok(scaled as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::{UiDrawBatch, UiPoint, UiVertex};

    #[test]
    fn fully_clipped_batches_are_dropped_and_remaining_indices_are_repacked() {
        let draw_list = UiDrawList {
            revision: 7,
            vertices: (0..8)
                .map(|index| UiVertex {
                    position: [index as f32, index as f32],
                    clip_z: 0.0,
                    clip_w: 1.0,
                    uv: [0.0, 0.0],
                    color: [255; 4],
                    style_flags: 0,
                    alpha_test: false,
                    alpha_cutoff: -1.0,
                    model_light: 1.0,
                    overlay_color: [0.0; 4],
                })
                .collect(),
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            batches: vec![
                UiDrawBatch {
                    texture_page: 0,
                    clip: rect(200.0, 200.0, 220.0, 220.0),
                    blend: ui::UiBlendMode::Alpha,
                    depth_test: false,
                    depth_write: false,
                    world_projection: false,
                    isolated_depth_scope: None,
                    index_range: 0..6,
                },
                UiDrawBatch {
                    texture_page: 0,
                    clip: rect(0.0, 0.0, 100.0, 100.0),
                    blend: ui::UiBlendMode::Invert,
                    depth_test: false,
                    depth_write: false,
                    world_projection: false,
                    isolated_depth_scope: None,
                    index_range: 6..12,
                },
            ],
        };

        let input = adapt_ui_draw_list(
            &draw_list,
            Arc::new(
                UiRenderTextureArray::new(
                    vec![render_model::UiTexturePage::owned([1, 1], vec![255; 4].into()).unwrap()],
                    1,
                )
                .unwrap(),
            ),
            UiRenderViewport {
                physical_size: [100, 100],
                dpi_scale: DpiScale::new(1.0).unwrap(),
                safe_area: SafeArea::ZERO,
            },
        )
        .unwrap();

        assert_eq!(&*input.indices, &[4, 5, 6, 4, 6, 7]);
        assert_eq!(input.batches.len(), 1);
        assert_eq!(input.batches[0].first_index, 0);
        assert_eq!(input.batches[0].index_count, 6);
        assert!(input.vertices.iter().all(|vertex| vertex.style_flags == 0));
        // The surviving batch keeps its declared blend on the render side.
        assert_eq!(input.batches[0].blend_mode, render_model::UI_BLEND_INVERT);
    }

    #[test]
    fn world_projection_keeps_homogeneous_depth_through_dpi_conversion() {
        let mut tree = ui::UiTree::new(vec![
            ui::UiNode::new(ui::UiNodeId::new(1), None, rect(-4.0, -2.0, 4.0, 2.0))
                .with_visual(ui::UiVisual::Solid {
                    texture_page: 0,
                    color: [0, 0, 0, 64],
                })
                .with_world_projection(ui::UiWorldProjection {
                    clip_from_local: [
                        [0.1, 0.0, 0.0, 0.0],
                        [0.0, -0.1, 0.0, 0.0],
                        [0.0; 4],
                        [0.0, 0.0, 0.5, 2.0],
                    ],
                    viewport_size: [100.0, 80.0],
                    depth_test: true,
                    depth_write: true,
                    alpha_test: true,
                }),
        ])
        .unwrap();
        tree.layout(
            rect(0.0, 0.0, 100.0, 80.0),
            ui::UiScale::new(2.0).unwrap(),
            SafeArea::new(8.0, 6.0, 0.0, 0.0).unwrap(),
        )
        .unwrap();
        let draw = tree.build_draw_list().unwrap();
        let input = adapt_ui_draw_list(
            &draw,
            Arc::new(
                UiRenderTextureArray::new(
                    vec![render_model::UiTexturePage::owned([1, 1], vec![255; 4].into()).unwrap()],
                    1,
                )
                .unwrap(),
            ),
            UiRenderViewport {
                physical_size: [200, 160],
                dpi_scale: DpiScale::new(2.0).unwrap(),
                safe_area: SafeArea::new(8.0, 6.0, 0.0, 0.0).unwrap(),
            },
        )
        .unwrap();
        assert_eq!(input.vertices[0].position, [160.0, 144.0]);
        assert_eq!(input.vertices[0].clip_z, 0.5);
        assert_eq!(input.vertices[0].clip_w, 2.0);
        assert!(
            input
                .vertices
                .iter()
                .all(|vertex| { vertex.style_flags & render_model::UI_STYLE_ALPHA_TEST != 0 })
        );
        assert_eq!(
            render_model::UI_STYLE_ALPHA_TEST
                & u32::from(ui::UI_STYLE_GRAYSCALE | ui::UI_STYLE_BILINEAR | ui::UI_STYLE_GLINT),
            0
        );
        assert_eq!(input.batches[0].depth_test, 1);
        assert_eq!(input.batches[0].depth_write, 1);
        assert_eq!(input.batches[0].world_projection, 1);
        assert_eq!(input.batches[0].scissor, UiScissor::new(0, 0, 200, 160));
    }

    #[test]
    fn model_mesh_dpi_keeps_material_cutoff_and_private_depth_without_world_projection() {
        let mesh = ui::UiMesh::new(
            [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]]
                .map(|position| ui::UiMeshVertex {
                    position,
                    clip_z: 0.75,
                    clip_w: 2.0,
                    uv: [8.5, 4.5],
                    color: [255; 4],
                    style_flags: 0,
                    alpha_test: false,
                    model_light: 0.718_629,
                    overlay_color: [0.8, 0.248_176, 0.0, 0.7],
                })
                .into(),
            vec![0, 1, 2].into(),
            vec![ui::UiMeshBatch {
                texture_page: 0,
                index_range: 0..3,
                blend: ui::UiBlendMode::Alpha,
                depth_test: true,
                depth_write: true,
                alpha_cutoff: Some(0.1),
            }]
            .into(),
        )
        .unwrap();
        let mut tree = ui::UiTree::new(vec![
            ui::UiNode::new(ui::UiNodeId::new(9), None, rect(10.0, 20.0, 30.0, 40.0))
                .with_visual(ui::UiVisual::Mesh(Arc::new(mesh))),
        ])
        .unwrap();
        tree.layout(
            rect(0.0, 0.0, 100.0, 100.0),
            ui::UiScale::default(),
            SafeArea::ZERO,
        )
        .unwrap();
        let input = adapt_ui_draw_list(
            &tree.build_draw_list().unwrap(),
            Arc::new(
                UiRenderTextureArray::new(
                    vec![render_model::UiTexturePage::owned([1, 1], vec![255; 4].into()).unwrap()],
                    1,
                )
                .unwrap(),
            ),
            UiRenderViewport {
                physical_size: [200, 200],
                dpi_scale: DpiScale::new(2.0).unwrap(),
                safe_area: SafeArea::ZERO,
            },
        )
        .unwrap();
        assert_eq!(input.vertices[0].position, [40.0, 80.0]);
        assert_eq!(input.vertices[0].clip_z, 0.75);
        assert_eq!(input.vertices[0].clip_w, 2.0);
        assert_eq!(input.vertices[0].alpha_cutoff, 0.1);
        assert_eq!(input.vertices[0].overlay_color, [0.8, 0.248_176, 0.0, 0.7]);
        assert_eq!(input.vertices[0].uv, [8.5, 4.5]);
        assert_eq!(
            input.vertices[0].model_light.to_bits(),
            0.718_629_f32.to_bits()
        );
        assert_eq!(input.batches[0].isolated_depth_scope, Some(9));
        assert_eq!(input.batches[0].world_projection, 0);
        assert_eq!(input.batches[0].depth_write, 1);
    }

    fn rect(left: f32, top: f32, right: f32, bottom: f32) -> UiRect {
        UiRect::new(
            UiPoint::new(left, top).unwrap(),
            UiPoint::new(right, bottom).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn review_negative_clip_coordinates_intersect_the_physical_viewport() {
        let clip = rect(-10.0, -20.0, 20.0, 30.0);
        assert_eq!(
            physical_scissor(clip, 2.0, [100, 100]).unwrap(),
            UiScissor::new(0, 0, 40, 60)
        );
    }
}
