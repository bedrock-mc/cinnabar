//! Bounds of retained, indexed artwork after its batch scissors are applied.

use super::{UiBlendMode, UiDrawList};
use crate::{UiPoint, UiRect};

impl UiDrawList {
    /// Conservative bounds of emitted triangles intersecting their scissors, without
    /// treating empty clip groups, unused vertices, or wholly clipped artwork as content.
    /// Fully transparent alpha paint is excluded; texture transparency is not sampled.
    /// Malformed geometry returns `None`.
    pub fn painted_bounds(&self) -> Option<UiRect> {
        let mut union: Option<[f32; 4]> = None;
        for batch in &self.batches {
            let indices = self
                .indices
                .get(batch.index_range.start as usize..batch.index_range.end as usize)?;
            for triangle in indices.as_chunks::<3>().0 {
                let mut low = [f32::INFINITY; 2];
                let mut high = [f32::NEG_INFINITY; 2];
                let mut ink = batch.blend != UiBlendMode::Alpha;
                for index in triangle {
                    let vertex = self.vertices.get(*index as usize)?;
                    if !vertex.clip_w.is_finite() || vertex.clip_w <= 0.0 {
                        return None;
                    }
                    for axis in 0..2 {
                        let value = vertex.position[axis] / vertex.clip_w;
                        if !value.is_finite() {
                            return None;
                        }
                        low[axis] = low[axis].min(value);
                        high[axis] = high[axis].max(value);
                    }
                    ink |= if vertex.style_flags & render_api::UI_STYLE_COVERAGE_PAINT != 0 {
                        let paint = render_api::UiCoveragePaint::from_vertex_colors(
                            vertex.color,
                            vertex.overlay_color,
                        );
                        paint.fill[0][3] != 0 || paint.fill[1][3] != 0 || paint.border[3] != 0
                    } else {
                        vertex.color[3] != 0
                    };
                }
                if !ink {
                    continue;
                }
                let bounds = [
                    low[0].max(batch.clip.min().x()),
                    low[1].max(batch.clip.min().y()),
                    high[0].min(batch.clip.max().x()),
                    high[1].min(batch.clip.max().y()),
                ];
                if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
                    continue;
                }
                union = Some(union.map_or(bounds, |prior| {
                    [
                        prior[0].min(bounds[0]),
                        prior[1].min(bounds[1]),
                        prior[2].max(bounds[2]),
                        prior[3].max(bounds[3]),
                    ]
                }));
            }
        }
        let [left, top, right, bottom] = union?;
        UiRect::new(
            UiPoint::new(left, top).ok()?,
            UiPoint::new(right, bottom).ok()?,
        )
        .ok()
    }
}

#[cfg(test)]
mod tests;
