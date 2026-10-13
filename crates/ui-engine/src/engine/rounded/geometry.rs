//! Screen-pixel coverage rings for one rounded fill and its inside border.

use std::sync::Arc;

use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex};

use super::Key;

const MAX_CORNER_STEPS: usize = 64;
const CHORD_ERROR_PIXELS: f32 = 0.125;
const COVERAGE_STEPS: usize = 4;

/// One logical-pixel width adjusted to remain one physical pixel at every DPI.
pub(super) fn coverage_width(dpi: f32) -> f32 {
    1.0 / dpi
}

struct Shape {
    width: f32,
    height: f32,
    radius: f32,
    fringe: f32,
    border: f32,
}

impl Shape {
    /// Keeps the coverage support centered on the authored outer and inner outlines.
    fn rings(&self) -> Vec<f32> {
        let limit = self.width.min(self.height) * 0.5;
        let mut rings = Vec::with_capacity(2 * (COVERAGE_STEPS + 1));
        for edge in [0.0, self.border] {
            if edge != 0.0 || rings.is_empty() {
                let steps = if edge == 0.0 && self.radius > 0.0 {
                    1
                } else {
                    COVERAGE_STEPS
                };
                for step in 0..=steps {
                    let inset = edge + self.fringe * (step as f32 / steps as f32 - 0.5);
                    rings.push(inset.min(limit));
                }
            }
        }
        rings.sort_by(f32::total_cmp);
        rings.dedup_by(|a, b| (*a - *b).abs() < self.fringe * 0.0001);
        rings
    }

    /// Chooses subpixel arc error with a fixed tessellation limit, including the coverage outset.
    fn corner_steps(&self) -> usize {
        let radius = self.radius / self.fringe + 0.5;
        let angle = 2.0 * (1.0 - CHORD_ERROR_PIXELS / radius).clamp(-1.0, 1.0).acos();
        let steps = (std::f32::consts::FRAC_PI_2 / angle)
            .ceil()
            .clamp(1.0, MAX_CORNER_STEPS as f32) as usize;
        steps.next_multiple_of(2)
    }

    /// An inset outline remains a real rectangle when its corner radius reaches zero.
    fn point(&self, inset: f32, corner: usize, direction: [f32; 2], progress: f32) -> [f32; 2] {
        let radius = if self.radius == 0.0 {
            0.0
        } else {
            (self.radius - inset).max(0.0)
        }
        .min((self.width.min(self.height) * 0.5 - inset).max(0.0));
        let edge = inset + radius;
        let center = [
            if matches!(corner, 0 | 3) {
                edge
            } else {
                self.width - edge
            },
            if corner < 2 { edge } else { self.height - edge },
        ];
        if radius == 0.0 {
            // Keep a corner sample and edge samples outside its two-axis coverage footprint.
            let band = [self.width, self.height]
                .map(|extent| self.fringe.min((extent * 0.5 - inset).max(0.0)));
            let before = (1.0 - 2.0 * progress).max(0.0);
            let after = (2.0 * progress - 1.0).max(0.0);
            let offset = match corner {
                0 => [after * band[0], before * band[1]],
                1 => [-before * band[0], after * band[1]],
                2 => [-after * band[0], -before * band[1]],
                _ => [before * band[0], -after * band[1]],
            };
            return [center[0] + offset[0], center[1] + offset[1]];
        }
        [
            center[0] + direction[0] * radius,
            center[1] + direction[1] * radius,
        ]
    }

    /// Extends an arc endpoint along its straight edge past the square inner corner's pixel band.
    fn straight_point(&self, mut point: [f32; 2], corner: usize, exit: bool) -> [f32; 2] {
        let axis = (corner + usize::from(!exit)) % 2;
        let extent = [self.width, self.height][axis];
        let support = (self.border + self.fringe * 0.5).min(extent * 0.5);
        point[axis] = point[axis].clamp(support, extent - support);
        point
    }

    /// Fill and border cover disjoint portions of one pixel, without an overlapping fill pass.
    fn coverage(&self, inset: f32) -> [f32; 2] {
        let extent = self.width.min(self.height);
        let ramp = |distance: f32, extent: f32| {
            (distance / self.fringe + 0.5)
                .clamp(0.0, 1.0)
                .min((extent / self.fringe).clamp(0.0, 1.0))
        };
        let outer = ramp(inset, extent);
        let fill = if self.border > 0.0 {
            ramp(inset - self.border, (extent - 2.0 * self.border).max(0.0)).min(outer)
        } else {
            outer
        };
        [fill, outer - fill]
    }

    /// Square corners integrate both pixel axes, including rectangles narrower than one pixel.
    fn coverage_at(&self, point: [f32; 2], inset: f32) -> [f32; 2] {
        let rect = |edge: f32| {
            let half = self.fringe * 0.5;
            [self.width, self.height]
                .into_iter()
                .zip(point)
                .map(|(extent, position)| {
                    (((extent - edge).min(position + half) - edge.max(position - half))
                        / self.fringe)
                        .clamp(0.0, 1.0)
                })
                .product::<f32>()
        };
        let mut coverage = self.coverage(inset);
        let outer = if self.radius == 0.0 {
            rect(0.0)
        } else {
            coverage[0] + coverage[1]
        };
        if self.border == 0.0 {
            coverage[0] = outer;
        } else if self.border >= self.radius {
            coverage[0] = rect(self.border).min(outer);
        }
        coverage[1] = (outer - coverage[0]).max(0.0);
        coverage
    }
}

/// Builds a single bounded, non-overlapping mesh whose edge coverage is independent of border width.
pub(super) fn mesh(key: Key) -> Option<UiMesh> {
    let [width, height, radius, fringe, border] =
        [key.width, key.height, key.radius, key.fringe, key.border].map(f32::from_bits);
    let shape = Shape {
        width,
        height,
        radius,
        fringe,
        border,
    };
    let rings = shape.rings();
    let steps = shape.corner_steps();
    let square_inner = radius > 0.0 && border >= radius;
    let count = 4 * (steps + 1 + 2 * usize::from(square_inner));
    let paint = render_api::UiCoveragePaint {
        fill: [key.color, key.end_color],
        border: key.border_color,
    };
    let (color, overlay_color) = paint.vertex_colors();
    let axis = usize::from(!key.horizontal);
    let vertex = |point: [f32; 2], inset: f32| {
        let [fill_area, border_area] = shape.coverage_at(point, inset);
        UiMeshVertex {
            position: [point[0] / width, point[1] / height],
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [point[axis] / [width, height][axis], fill_area],
            color,
            model_light: border_area,
            overlay_color,
            style_flags: render_api::UI_STYLE_COVERAGE_PAINT,
            alpha_test: false,
        }
    };
    let center = [width * 0.5, height * 0.5];
    let mut vertices = Vec::with_capacity(1 + rings.len() * count);
    vertices.push(vertex(center, width.min(height) * 0.5));
    for &inset in &rings {
        for corner in 0..4 {
            let start = std::f32::consts::PI + corner as f32 * std::f32::consts::FRAC_PI_2;
            for step in 0..=steps {
                let angle = start + step as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
                let point = shape.point(
                    inset,
                    corner,
                    [angle.cos(), angle.sin()],
                    step as f32 / steps as f32,
                );
                if square_inner && step == 0 {
                    vertices.push(vertex(shape.straight_point(point, corner, false), inset));
                }
                vertices.push(vertex(point, inset));
                if square_inner && step == steps {
                    vertices.push(vertex(shape.straight_point(point, corner, true), inset));
                }
            }
        }
    }
    let mut indices = Vec::with_capacity(count * (6 * (rings.len() - 1) + 3));
    for ring in 0..rings.len() - 1 {
        for point in 0..count {
            let here = (1 + ring * count + point) as u32;
            let next = (1 + ring * count + (point + 1) % count) as u32;
            let inner = here + count as u32;
            let inner_next = next + count as u32;
            indices.extend([here, next, inner_next, here, inner_next, inner]);
        }
    }
    let last = 1 + (rings.len() - 1) * count;
    for point in 0..count {
        indices.extend([
            0,
            (last + point) as u32,
            (last + (point + 1) % count) as u32,
        ]);
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
mod tests;
