//! Retained placement for a guest-authored settings card in GUI pixels.

use super::EDITOR_MARGIN;

const GAP: f64 = 16.;

#[derive(Clone, Copy, PartialEq)]
struct Inputs {
    selection: [f64; 4],
    viewport: [f64; 2],
    size: [f64; 2],
}

#[derive(Default)]
pub(super) struct Placement {
    inputs: Option<Inputs>,
    pub at: [f64; 2],
}

impl Placement {
    /// Repositions only when displayed selection bounds, viewport or authored size changes.
    pub fn update(
        &mut self,
        selection: Option<[f64; 4]>,
        viewport: [f64; 2],
        size: Option<[f32; 2]>,
    ) -> bool {
        let inputs = selection.zip(size).map(|(selection, size)| Inputs {
            selection,
            viewport,
            size: size.map(f64::from),
        });
        if self.inputs == inputs {
            return false;
        }
        self.inputs = inputs;
        self.at = inputs.map_or([0.; 2], Inputs::place);
        true
    }
}

impl Inputs {
    /// Clamps even oversized chrome without inverted ranges or negative coordinates.
    fn clamp(self, at: [f64; 2]) -> [f64; 2] {
        std::array::from_fn(|axis| {
            let available = (self.viewport[axis] - self.size[axis]).max(0.);
            let margin = EDITOR_MARGIN.min(available * 0.5);
            at[axis].clamp(margin, available - margin)
        })
    }

    /// Tests actual coverage after clamping rather than assuming a preferred side fits.
    fn overlaps(self, at: [f64; 2]) -> bool {
        (0..2).all(|axis| {
            at[axis] < self.selection[axis] + self.selection[axis + 2]
                && at[axis] + self.size[axis] > self.selection[axis]
        })
    }

    /// Prefers right then left, using free vertical space if neither side can avoid coverage.
    fn place(self) -> [f64; 2] {
        let [x, y, width, height] = self.selection;
        let right = [x + width + GAP, y];
        let left = [x - GAP - self.size[0], y];
        let preferred = self.clamp(
            if right[0] + self.size[0] <= self.viewport[0] - EDITOR_MARGIN {
                right
            } else {
                left
            },
        );
        if !self.overlaps(preferred) {
            return preferred;
        }
        [
            right,
            left,
            [x, y + height + GAP],
            [x, y - GAP - self.size[1]],
        ]
        .into_iter()
        .map(|at| self.clamp(at))
        .find(|at| !self.overlaps(*at))
        .unwrap_or(preferred)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amendment_inspector_prefers_right_then_left_and_clamps_vertically() {
        let mut placement = Placement::default();
        for (selection, expected) in [
            ([40., 60., 100., 30.], [156., 60.]),
            ([700., 60., 100., 30.], [504., 60.]),
            ([40., 550., 100., 30.], [156., 472.]),
            ([0., 0., 100., 30.], [116., EDITOR_MARGIN]),
        ] {
            assert!(placement.update(Some(selection), [900., 600.], Some([180., 120.])));
            assert_eq!(placement.at, expected);
        }
    }

    #[test]
    fn amendment_inspector_uses_free_vertical_space_and_tolerates_oversized_chrome() {
        let mut placement = Placement::default();
        for (selection, size, expected) in [
            ([100., 50., 200., 30.], [260., 100.], [100., 96.]),
            ([100., 320., 200., 30.], [260., 100.], [100., 204.]),
            ([100., 150., 200., 30.], [500., 500.], [0., 0.]),
        ] {
            placement.update(Some(selection), [400., 400.], Some(size));
            assert_eq!(placement.at, expected);
        }
        // A clamped side can still avoid coverage even when the full 16 px gap does not fit.
        placement.update(
            Some([100., 100., 100., 30.]),
            [400., 400.],
            Some([190., 100.]),
        );
        assert_eq!(placement.at, [202., 100.]);
    }

    #[test]
    fn amendment_inspector_recomputes_only_when_its_geometry_changes() {
        let mut placement = Placement::default();
        let rect = Some([40., 60., 100., 30.]);
        let viewport = [900., 600.];
        let size = Some([180., 120.]);
        assert!(placement.update(rect, viewport, size));
        let (_, allocations) = test_alloc::measure(|| {
            for _ in 0..100 {
                assert!(!placement.update(rect, viewport, size));
            }
        });
        assert_eq!(allocations.calls, 0);
        assert!(placement.update(rect, [400., 300.], size));
        assert!(placement.update(rect, [400., 300.], Some([200., 140.])));
        assert!(placement.update(Some([80., 60., 100., 30.]), [400., 300.], size));
        assert!(placement.update(None, viewport, size));
        assert_eq!(placement.at, [0.; 2]);
        assert!(!placement.update(None, [400., 300.], size));
        assert!(placement.update(rect, viewport, size));
        assert!(placement.update(rect, viewport, None));
        assert_eq!(placement.at, [0.; 2]);
    }
}
