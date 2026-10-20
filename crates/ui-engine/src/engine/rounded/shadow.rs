//! A cached analytic shadow quad with color and translation kept outside its geometry key.

use super::{Canvas, MeshCache, geometry::coverage_width};
use crate::engine::screen::PaintError;
use json_ui::rounded::{DEFAULT_RADIUS, SHADOW_SUPPORT, Shadow};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};
use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex, UiVisual};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Key {
    size: [u32; 2],
    radius: u32,
    blur: u32,
    fringe: u32,
    page: u16,
}

thread_local! { static SHADOWS: RefCell<MeshCache<Key>> = RefCell::default(); }

impl Canvas<'_> {
    /// Paints an enabled shadow before the rounded fill, inside the ancestor clip group.
    pub(in crate::engine) fn rounded_shadow(
        &mut self,
        data: &BTreeMap<String, Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Result<(), PaintError> {
        if let Some((visual, bounds)) = visual(
            data,
            dest,
            self.px * self.length_scale,
            coverage_width(self.metrics.dpi_scale.get()),
            self.solid_page,
            alpha,
        ) {
            self.push(visual, bounds)?;
        }
        Ok(())
    }
}

/// Resolves changing paint separately from the immutable, bounded geometry cache.
fn visual(
    data: &BTreeMap<String, Value>,
    dest: [f32; 4],
    scale: f32,
    fringe: f32,
    page: u16,
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) -> Option<(UiVisual, [f32; 4])> {
    let shadow = Shadow::read(data)?;
    let color = alpha(shadow.color);
    if color[3] == 0 {
        return None;
    }
    let original = [dest[2] - dest[0], dest[3] - dest[1]];
    let spread = shadow.spread * scale;
    let size = original.map(|axis| axis + spread * 2.0);
    if original.iter().chain(size.iter()).any(|axis| *axis <= 0.0) {
        return None;
    }
    let radius = data
        .get("radius")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(f64::from(DEFAULT_RADIUS)) as f32
        * scale;
    let radius = (radius.clamp(0.0, original[0].min(original[1]) * 0.5) + spread)
        .clamp(0.0, size[0].min(size[1]) * 0.5);
    let blur = shadow.blur * scale;
    let padding = (blur * SHADOW_SUPPORT).max(fringe * 0.5);
    let bounds = [
        dest[0] + shadow.offset[0] * scale - spread - padding,
        dest[1] + shadow.offset[1] * scale - spread - padding,
        dest[2] + shadow.offset[0] * scale + spread + padding,
        dest[3] + shadow.offset[1] * scale + spread + padding,
    ];
    let key = Key {
        size: size.map(f32::to_bits),
        radius: radius.to_bits(),
        blur: blur.to_bits(),
        fringe: fringe.to_bits(),
        page,
    };
    let mesh = SHADOWS.with(|cache| cache.borrow_mut().get_or_build(key, mesh))?;
    Some((UiVisual::TintedMesh { mesh, color }, bounds))
}

/// One quad carries the local rounded-rect distance parameters to the existing UI shader.
fn mesh(key: Key) -> Option<UiMesh> {
    let half = key.size.map(|axis| f32::from_bits(axis) * 0.5);
    let blur = f32::from_bits(key.blur);
    let padding = (blur * SHADOW_SUPPORT).max(f32::from_bits(key.fringe) * 0.5);
    let vertices = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]].map(|position| UiMeshVertex {
        position,
        clip_z: 0.0,
        clip_w: 1.0,
        uv: std::array::from_fn(|axis| (position[axis] * 2.0 - 1.0) * (half[axis] + padding)),
        color: [255; 4],
        model_light: 1.0,
        overlay_color: [half[0], half[1], f32::from_bits(key.radius), blur],
        style_flags: render_api::ui_shadow::UI_STYLE_SHADOW,
        alpha_test: false,
    });
    UiMesh::new(
        Arc::from(vertices),
        Arc::from([0, 1, 2, 0, 2, 3]),
        Arc::from([UiMeshBatch {
            texture_page: key.page,
            index_range: 0..6,
            blend: UiBlendMode::Alpha,
            depth_test: false,
            depth_write: false,
            alpha_cutoff: None,
        }]),
    )
    .ok()
}

#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod tests;
