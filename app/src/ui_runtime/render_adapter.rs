use std::{fmt, sync::Arc};

use render::{
    UiRenderBatch, UiRenderInput, UiRenderRejectReason, UiRenderTextureArray, UiRenderVertex,
    UiScissor,
};
use ui::{DpiScale, SafeArea, UiDrawList, UiLimits, UiRect};

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
                uv: vertex.uv,
                color: vertex.color,
                style_flags: u32::from(vertex.style_flags),
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
        batches.push(UiRenderBatch::new(
            u32::from(batch.texture_page),
            scissor,
            first_index,
            index_count,
            match batch.blend {
                ui::UiBlendMode::Alpha => render::UI_BLEND_ALPHA,
                ui::UiBlendMode::Invert => render::UI_BLEND_INVERT,
            },
        ));
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
    let left = scaled_floor(clip.min().x(), scale)?;
    let top = scaled_floor(clip.min().y(), scale)?;
    let right = scaled_ceil(clip.max().x(), scale)?.min(viewport[0]);
    let bottom = scaled_ceil(clip.max().y(), scale)?.min(viewport[1]);
    let left = left.min(viewport[0]);
    let top = top.min(viewport[1]);
    Ok(UiScissor::new(
        left,
        top,
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    ))
}

fn physical_safe_area(safe_area: SafeArea, scale: f32) -> Result<[u32; 4], UiRenderAdapterError> {
    Ok([
        scaled_ceil(safe_area.left(), scale)?,
        scaled_ceil(safe_area.top(), scale)?,
        scaled_ceil(safe_area.right(), scale)?,
        scaled_ceil(safe_area.bottom(), scale)?,
    ])
}

fn scaled_floor(value: f32, scale: f32) -> Result<u32, UiRenderAdapterError> {
    scaled_u32(value, scale, f32::floor)
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
                    uv: [0, 0],
                    color: [255; 4],
                    style_flags: 0,
                })
                .collect(),
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            batches: vec![
                UiDrawBatch {
                    texture_page: 0,
                    clip: rect(200.0, 200.0, 220.0, 220.0),
                    blend: ui::UiBlendMode::Alpha,
                    index_range: 0..6,
                },
                UiDrawBatch {
                    texture_page: 0,
                    clip: rect(0.0, 0.0, 100.0, 100.0),
                    blend: ui::UiBlendMode::Invert,
                    index_range: 6..12,
                },
            ],
        };

        let input = adapt_ui_draw_list(
            &draw_list,
            Arc::new(UiRenderTextureArray {
                identity: [1; 32],
                width: 1,
                height: 1,
                layers: 1,
                rgba8: vec![255; 4].into(),
            }),
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
        // The surviving batch keeps its declared blend on the render side.
        assert_eq!(input.batches[0].blend_mode, render::UI_BLEND_INVERT);
    }

    fn rect(left: f32, top: f32, right: f32, bottom: f32) -> UiRect {
        UiRect::new(
            UiPoint::new(left, top).unwrap(),
            UiPoint::new(right, bottom).unwrap(),
        )
        .unwrap()
    }
}
