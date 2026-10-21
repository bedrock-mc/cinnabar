//! Optional bounds observation of actual painting, including custom renderer overflow.

use super::Canvas;
use std::cell::Cell;

/// A bounded union of logical painting rectangles, shared by native and subtree observers.
/// Rectangles use `[left, top, right, bottom]` in logical pixels.
#[derive(Default)]
pub struct PaintBounds(Cell<Option<[f32; 4]>>);

impl PaintBounds {
    /// Adds one painted rectangle without retaining its geometry.
    pub fn include(&self, bounds: [f32; 4]) {
        self.0.set(Some(self.0.get().map_or(bounds, |old| {
            [
                old[0].min(bounds[0]),
                old[1].min(bounds[1]),
                old[2].max(bounds[2]),
                old[3].max(bounds[3]),
            ]
        })));
    }

    /// Returns the current union, or none when no artwork was painted.
    pub fn get(&self) -> Option<[f32; 4]> {
        self.0.get()
    }
}

/// Selects screen artwork whose actual painting contributes to one shared bounds collector.
#[derive(Clone, Copy)]
pub struct PaintObserver<'a> {
    pub bounds: &'a PaintBounds,
    pub region: std::num::NonZeroU64,
}

impl Canvas<'_> {
    /// Captures primitive bounds passed to `push`, after the canvas transform and clip.
    /// Observation allocates nothing and does not inspect texture transparency or visual coverage.
    /// Nested observers retain the enclosing observer's bounds as well.
    pub fn capture_paint_bounds<R>(
        &mut self,
        paint: impl FnOnce(&mut Self) -> R,
    ) -> (R, Option<[f32; 4]>) {
        let previous = self.paint_bounds.replace(None);
        let result = paint(self);
        let bounds = self.paint_bounds.take().flatten();
        self.paint_bounds = previous;
        if let Some(bounds) = bounds {
            self.extend_paint_bounds(bounds);
        }
        (result, bounds)
    }

    /// Observes a pushed primitive in the same residual transform and clip used for painting.
    pub(super) fn observe_paint_bounds(&mut self, bounds: [f32; 4], clip: [f32; 4]) {
        if self.paint_bounds.is_none() {
            return;
        }
        let rect = json_ui::Rect::new(
            f64::from(bounds[0]),
            f64::from(bounds[1]),
            f64::from(bounds[2] - bounds[0]),
            f64::from(bounds[3] - bounds[1]),
        );
        let rect = self
            .residual
            .map_or(rect, |transform| transform.bounds(rect));
        let bounds = [
            (rect.x as f32).max(clip[0]),
            (rect.y as f32).max(clip[1]),
            ((rect.x + rect.w) as f32).min(clip[2]),
            ((rect.y + rect.h) as f32).min(clip[3]),
        ];
        if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
            self.extend_paint_bounds(bounds);
        }
    }

    /// Extends only an active observer, preserving scopes that emitted no visible primitives.
    fn extend_paint_bounds(&mut self, bounds: [f32; 4]) {
        if let Some(current) = &mut self.paint_bounds {
            *current = Some(current.map_or(bounds, |old| {
                [
                    old[0].min(bounds[0]),
                    old[1].min(bounds[1]),
                    old[2].max(bounds[2]),
                    old[3].max(bounds[3]),
                ]
            }));
        }
    }
}
