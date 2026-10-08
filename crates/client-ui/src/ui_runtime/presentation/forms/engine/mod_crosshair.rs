//! Cached cosmetic crosshair coverage drawn only by a visible JSON-UI cursor renderer.

use super::Painter;
use std::{cell::RefCell, collections::HashMap, sync::Arc};
use ui::{
    UiBlendMode, UiLimits, UiMesh, UiMeshBatch, UiMeshVertex, UiVisual,
    mod_hud::{Crosshair, CrosshairShape},
};

const MAX_CACHED_CROSSHAIRS: usize = 16;
// Draw emission expands each triangle corner into one retained vertex.
const MAX_GRID_CELLS: usize = (UiLimits::MAX_UI_VERTICES / 6).isqrt();
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Key {
    shape: CrosshairShape,
    size: u32,
    gap: u32,
    thickness: u32,
    outline: u32,
    px: u32,
    color: [u8; 4],
    outline_color: [u8; 4],
    page: u16,
}
thread_local! { static SHAPES: RefCell<HashMap<Key, Arc<UiMesh>>> = RefCell::default(); }

impl Painter<'_> {
    /// Returns false if the replacement cannot draw, so ordinary cursor art survives.
    pub(super) fn mod_crosshair(&mut self, spec: &Crosshair, dest: [f32; 4]) -> bool {
        let rgba = |color: [f32; 4]| color.map(|v| (v * 255.).round() as u8);
        let key = Key {
            shape: spec.shape,
            size: spec.size.to_bits(),
            gap: spec.gap.to_bits(),
            thickness: spec.thickness.to_bits(),
            outline: spec.outline.to_bits(),
            px: self.px.to_bits(),
            color: rgba(spec.color),
            outline_color: rgba(spec.outline_color),
            page: self.solid_page,
        };
        let mesh = SHAPES.with(|shapes| {
            let mut shapes = shapes.borrow_mut();
            if let Some(mesh) = shapes.get(&key) {
                return Some(Arc::clone(mesh));
            }
            let mesh = Arc::new(mesh(key)?);
            if shapes.len() >= MAX_CACHED_CROSSHAIRS {
                shapes.clear();
            }
            shapes.insert(key, Arc::clone(&mesh));
            Some(mesh)
        });
        let Some(mesh) = mesh else {
            return false;
        };
        let side = extent(key) * 2.;
        let x = (dest[0] + dest[2] - side) * 0.5;
        let y = (dest[1] + dest[3] - side) * 0.5;
        self.push(UiVisual::Mesh(mesh), [x, y, x + side, y + side])
            .is_ok()
    }
}
fn extent(key: Key) -> f32 {
    let [size, gap, thickness, outline, px] =
        [key.size, key.gap, key.thickness, key.outline, key.px].map(f32::from_bits);
    (size
        + if key.shape == CrosshairShape::Cross {
            gap
        } else {
            0.
        }
        + thickness * 0.5
        + outline)
        * px
        + 1.
}
fn box_distance(point: [f32; 2], center: [f32; 2], half: [f32; 2]) -> f32 {
    let q = [
        (point[0] - center[0]).abs() - half[0],
        (point[1] - center[1]).abs() - half[1],
    ];
    q[0].max(0.).hypot(q[1].max(0.)) + q[0].max(q[1]).min(0.)
}
fn distance(key: Key, point: [f32; 2]) -> f32 {
    let [size, gap, thickness, px] = [key.size, key.gap, key.thickness, key.px].map(f32::from_bits);
    let [size, gap, thickness] = [size * px, gap * px, thickness * px];
    match key.shape {
        CrosshairShape::Dot => point[0].hypot(point[1]) - size,
        CrosshairShape::Circle => (point[0].hypot(point[1]) - size).abs() - thickness * 0.5,
        CrosshairShape::Cross => {
            let center = gap + size * 0.5;
            let half = [size * 0.5, thickness * 0.5];
            [
                box_distance(point, [center, 0.], half),
                box_distance(point, [-center, 0.], half),
                box_distance([point[1], point[0]], [center, 0.], half),
                box_distance([point[1], point[0]], [-center, 0.], half),
            ]
            .into_iter()
            .fold(f32::INFINITY, f32::min)
        }
    }
}
fn coverage_color(key: Key, point: [f32; 2]) -> [u8; 4] {
    let d = distance(key, point);
    let outline = f32::from_bits(key.outline) * f32::from_bits(key.px);
    let foreground = (0.5 - d).clamp(0., 1.) * f32::from(key.color[3]) / 255.;
    let background = if outline > 0. {
        (0.5 - d + outline).clamp(0., 1.) * f32::from(key.outline_color[3]) / 255.
            * (1. - foreground)
    } else {
        0.
    };
    let alpha = foreground + background;
    if alpha <= 0. {
        return [0; 4];
    }
    let mut color = [0; 4];
    for (channel, value) in color.iter_mut().take(3).enumerate() {
        *value = ((f32::from(key.color[channel]) * foreground
            + f32::from(key.outline_color[channel]) * background)
            / alpha)
            .round() as u8;
    }
    color[3] = (alpha * 255.).round() as u8;
    color
}
fn mesh(key: Key) -> Option<UiMesh> {
    let half = extent(key);
    let side = half * 2.;
    if !side.is_finite() || side <= 0. {
        return None;
    }
    let cells = (side.ceil() as usize).clamp(4, MAX_GRID_CELLS);
    let mut vertices = Vec::with_capacity((cells + 1) * (cells + 1));
    for y in 0..=cells {
        for x in 0..=cells {
            let position = [x as f32 / cells as f32, y as f32 / cells as f32];
            vertices.push(UiMeshVertex {
                position,
                clip_z: 0.,
                clip_w: 1.,
                uv: [0.; 2],
                color: coverage_color(key, [position[0] * side - half, position[1] * side - half]),
                model_light: 1.,
                overlay_color: [0.; 4],
                style_flags: 0,
                alpha_test: false,
            });
        }
    }
    let mut indices = Vec::new();
    for y in 0..cells {
        for x in 0..cells {
            let a = (y * (cells + 1) + x) as u32;
            let corners = [a, a + 1, a + cells as u32 + 1, a + cells as u32 + 2];
            if corners.iter().all(|&i| vertices[i as usize].color[3] == 0) {
                continue;
            }
            indices.extend([
                corners[0], corners[1], corners[3], corners[0], corners[3], corners[2],
            ]);
        }
    }
    let end = indices.len() as u32;
    UiMesh::new(
        vertices.into(),
        indices.into(),
        Arc::from([UiMeshBatch {
            texture_page: key.page,
            index_range: 0..end,
            blend: UiBlendMode::Alpha,
            depth_test: false,
            depth_write: false,
            alpha_cutoff: None,
        }]),
    )
    .ok()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn key(shape: CrosshairShape, px: f32) -> Key {
        Key {
            shape,
            size: 4_f32.to_bits(),
            gap: 2_f32.to_bits(),
            thickness: 1_f32.to_bits(),
            outline: 1_f32.to_bits(),
            px: px.to_bits(),
            color: [255; 4],
            outline_color: [0, 0, 0, 255],
            page: 0,
        }
    }
    #[test]
    fn cross_preserves_clear_center_and_arms_across_scales() {
        for px in [1., 1.5, 2., 3., 4.] {
            let key = key(CrosshairShape::Cross, px);
            assert_eq!(coverage_color(key, [0.; 2])[3], 0);
            assert_eq!(coverage_color(key, [4. * px, 0.]), [255; 4]);
            assert_eq!(coverage_color(key, [0., 4. * px]), [255; 4]);
            assert!(mesh(key).is_some());
        }
    }
    #[test]
    fn dot_fills_center_and_ring_keeps_it_clear() {
        let dot = key(CrosshairShape::Dot, 2.);
        let ring = key(CrosshairShape::Circle, 2.);
        assert_eq!(coverage_color(dot, [0.; 2]), [255; 4]);
        assert_eq!(coverage_color(ring, [0.; 2])[3], 0);
        assert_eq!(coverage_color(ring, [8., 0.]), [255; 4]);
        assert!(coverage_color(ring, [9.5, 0.])[3] > 0);
        assert_eq!(coverage_color(ring, [15., 0.])[3], 0);
    }
    #[test]
    fn maximum_crosshair_geometry_fits_the_mesh_budget_at_maximum_gui_scale() {
        let px = ui::gui_scale([7680, 4320], None) as f32;
        for shape in [
            CrosshairShape::Cross,
            CrosshairShape::Dot,
            CrosshairShape::Circle,
        ] {
            for thickness in [0.5_f32, 8.] {
                for outline in [0_f32, 4.] {
                    let key = Key {
                        size: 16_f32.to_bits(),
                        gap: 12_f32.to_bits(),
                        thickness: thickness.to_bits(),
                        outline: outline.to_bits(),
                        ..key(shape, px)
                    };
                    let mesh = mesh(key)
                        .expect("valid cosmetic geometry stays within the host mesh limits");
                    assert!(!mesh.indices().is_empty());
                    assert!(mesh.indices().len() <= UiLimits::MAX_UI_VERTICES);
                    assert!(mesh.vertices().len() <= UiLimits::MAX_UI_VERTICES);
                }
            }
        }
    }
}
