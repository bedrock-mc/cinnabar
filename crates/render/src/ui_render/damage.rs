//! Conservative pixel damage for a retained UI layer with an unchanged draw topology.

use std::{mem::size_of_val, sync::Arc};

use render_model::{
    MAX_UI_BATCHES, MAX_UI_DRAW_BYTES, MAX_UI_INDICES, MAX_UI_VERTICES, UI_BLEND_ALPHA,
    UI_STYLE_GLINT, UiRenderInput, UiRenderVertex, UiScissor,
};

/// The pixels that must be restored and replayed before retaining the current publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UiDamage {
    Full,
    Unchanged,
    Rect(UiScissor),
}

/// Finds old and new coverage of changed triangles under the default full-target viewport.
/// Callers must replay every overlapping batch in order, including unchanged depth-writing models.
pub(super) fn plan(previous: &UiRenderInput, current: &UiRenderInput) -> UiDamage {
    if previous.viewport_size != current.viewport_size
        || previous.safe_area != current.safe_area
        || !Arc::ptr_eq(&previous.textures, &current.textures)
        || previous.indices != current.indices
        || previous.batches != current.batches
        || previous.vertices.len() != current.vertices.len()
        || !supported(previous)
        || !supported(current)
    {
        return UiDamage::Full;
    }
    if bytemuck::cast_slice::<_, u8>(&previous.vertices)
        == bytemuck::cast_slice::<_, u8>(&current.vertices)
    {
        return UiDamage::Unchanged;
    }
    let mut damage = None;
    for batch in current.batches.iter() {
        let start = batch.first_index as usize;
        let end = start + batch.index_count as usize;
        for triangle in current.indices[start..end].chunks_exact(3) {
            if triangle.iter().all(|&index| {
                bytemuck::bytes_of(&previous.vertices[index as usize])
                    == bytemuck::bytes_of(&current.vertices[index as usize])
            }) {
                continue;
            }
            let mut bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for input in [previous, current] {
                for &index in triangle {
                    let vertex = &input.vertices[index as usize];
                    for axis in 0..2 {
                        let pixel = f64::from(vertex.position[axis]) / f64::from(vertex.clip_w);
                        bounds[axis] = bounds[axis].min(pixel);
                        bounds[axis + 2] = bounds[axis + 2].max(pixel);
                    }
                }
            }
            if let Some(rect) = clipped(bounds, batch.scissor, current.viewport_size) {
                damage = Some(match damage {
                    Some(previous) => union(previous, rect),
                    None => rect,
                });
            }
        }
    }
    damage.map_or(UiDamage::Unchanged, UiDamage::Rect)
}

/// Rejects inputs whose shader coverage or ordered depth lifetimes cannot be bounded safely.
fn supported(input: &UiRenderInput) -> bool {
    let [width, height] = input.viewport_size;
    if width == 0
        || height == 0
        || input.vertices.len() > MAX_UI_VERTICES
        || input.indices.len() > MAX_UI_INDICES
        || input.batches.len() > MAX_UI_BATCHES
        || size_of_val(input.vertices.as_ref())
            + size_of_val(input.indices.as_ref())
            + size_of_val(input.batches.as_ref())
            > MAX_UI_DRAW_BYTES
        || input.safe_area[0]
            .checked_add(input.safe_area[2])
            .is_none_or(|sum| sum > width)
        || input.safe_area[1]
            .checked_add(input.safe_area[3])
            .is_none_or(|sum| sum > height)
        || input
            .vertices
            .iter()
            .any(|vertex| !supported_vertex(vertex, input.viewport_size))
        || input
            .indices
            .iter()
            .any(|&index| index as usize >= input.vertices.len())
    {
        return false;
    }
    let mut end = 0;
    let mut scope = None;
    for (index, batch) in input.batches.iter().enumerate() {
        let scissor = batch.scissor;
        if batch.first_index as usize != end
            || batch.index_count == 0
            || !batch.index_count.is_multiple_of(3)
            || batch.texture_page as usize >= input.textures.pages().len()
            || batch.world_projection != 0
            || batch.blend_mode != UI_BLEND_ALPHA
            || batch.depth_test > 1
            || batch.depth_write > 1
            || ((batch.depth_test != 0 || batch.depth_write != 0)
                && batch.isolated_depth_scope.is_none())
            || scissor.width == 0
            || scissor.height == 0
            || scissor
                .x
                .checked_add(scissor.width)
                .is_none_or(|right| right > width)
            || scissor
                .y
                .checked_add(scissor.height)
                .is_none_or(|bottom| bottom > height)
        {
            return false;
        }
        if batch.isolated_depth_scope != scope {
            scope = batch.isolated_depth_scope;
            if scope.is_some()
                && input.batches[..index]
                    .iter()
                    .any(|earlier| earlier.isolated_depth_scope == scope)
            {
                return false;
            }
        }
        let Some(next) = end.checked_add(batch.index_count as usize) else {
            return false;
        };
        if next > input.indices.len() {
            return false;
        }
        end = next;
    }
    end == input.indices.len()
}

/// Positive homogeneous W bounds clipped triangles by their projected vertices.
fn supported_vertex(vertex: &UiRenderVertex, viewport: [u32; 2]) -> bool {
    vertex.clip_w.is_normal()
        && vertex.clip_w > 0.0
        && vertex.clip_z.is_finite()
        && (vertex.clip_z / vertex.clip_w).is_finite()
        && vertex.uv.iter().all(|value| value.is_finite())
        && vertex.alpha_cutoff.is_finite()
        && vertex.alpha_cutoff <= 1.0
        && vertex.model_light.is_finite()
        && vertex.model_light >= 0.0
        && vertex.overlay_color.iter().all(|value| value.is_finite())
        && vertex.style_flags & UI_STYLE_GLINT == 0
        && (0..2).all(|axis| {
            let position = vertex.position[axis];
            let scaled = position / viewport[axis] as f32;
            let clip = scaled * 2.0 - vertex.clip_w;
            // Shaders may flush subnormal inputs or intermediates to zero.
            (position == 0.0 || (position.is_normal() && scaled.is_normal()))
                && (clip == 0.0 || clip.is_normal())
                && (position / vertex.clip_w).is_finite()
                && (clip / vertex.clip_w).is_finite()
        })
}

/// Adds a pixel for shader projection rounding, then clips outward-rounded coverage.
fn clipped(bounds: [f64; 4], scissor: UiScissor, viewport: [u32; 2]) -> Option<UiScissor> {
    let left = (bounds[0].floor() - 1.0).max(f64::from(scissor.x)).max(0.0);
    let top = (bounds[1].floor() - 1.0).max(f64::from(scissor.y)).max(0.0);
    let right = (bounds[2].ceil() + 1.0)
        .min(f64::from(scissor.x + scissor.width))
        .min(f64::from(viewport[0]));
    let bottom = (bounds[3].ceil() + 1.0)
        .min(f64::from(scissor.y + scissor.height))
        .min(f64::from(viewport[1]));
    (left < right && top < bottom).then(|| {
        UiScissor::new(
            left as u32,
            top as u32,
            right as u32 - left as u32,
            bottom as u32 - top as u32,
        )
    })
}

/// Unites already clipped rectangles without losing pixels between separate changes.
fn union(a: UiScissor, b: UiScissor) -> UiScissor {
    let left = a.x.min(b.x);
    let top = a.y.min(b.y);
    let right = (a.x + a.width).max(b.x + b.width);
    let bottom = (a.y + a.height).max(b.y + b.height);
    UiScissor::new(left, top, right - left, bottom - top)
}

#[cfg(test)]
#[path = "damage/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "damage/raster_tests.rs"]
mod raster_tests;
