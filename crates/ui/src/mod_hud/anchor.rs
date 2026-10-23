//! GUI-pixel placement shared by HUD rendering and completed editor gestures.

use super::Anchor;

impl Anchor {
    /// Row-major order used by the editor's nine-position anchor picker.
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
    ];

    /// Insets edge axes by the editor margin and leaves centered axes centered.
    pub fn editor_offset(self, margin: f32) -> [f32; 2] {
        self.fractions()
            .map(|fraction| if fraction == 0.5 { 0. } else { margin })
    }

    /// Returns each axis's matching card and viewport reference point.
    fn fractions(self) -> [f64; 2] {
        match self {
            Self::TopLeft => [0., 0.],
            Self::TopCenter => [0.5, 0.],
            Self::TopRight => [1., 0.],
            Self::Left => [0., 0.5],
            Self::Center => [0.5, 0.5],
            Self::Right => [1., 0.5],
            Self::BottomLeft => [0., 1.],
            Self::BottomCenter => [0.5, 1.],
            Self::BottomRight => [1., 1.],
        }
    }

    /// Resolves inward edge offsets and signed center offsets, clamping to available travel.
    pub fn origin(self, offset: [f32; 2], size: [f64; 2], viewport: [f64; 2]) -> [f64; 2] {
        let at = self.origin_unclamped(offset, size, viewport);
        std::array::from_fn(|axis| at[axis].clamp(0., (viewport[axis] - size[axis]).max(0.)))
    }

    /// Inverts authored offsets without moving a host factory control that overhangs the viewport.
    pub fn origin_unclamped(
        self,
        offset: [f32; 2],
        size: [f64; 2],
        viewport: [f64; 2],
    ) -> [f64; 2] {
        let fractions = self.fractions();
        std::array::from_fn(|axis| {
            let available = (viewport[axis] - size[axis]).max(0.);
            let direction = if fractions[axis] == 1. { -1. } else { 1. };
            available * fractions[axis] + f64::from(offset[axis]) * direction
        })
    }

    /// Chooses viewport thirds using the card center and preserves its current GUI-pixel origin.
    pub fn from_origin(at: [f64; 2], size: [f64; 2], viewport: [f64; 2]) -> (Self, [f32; 2]) {
        let thirds: [u8; 2] = std::array::from_fn(|axis| {
            let center = at[axis] + size[axis] * 0.5;
            if center < viewport[axis] / 3. {
                0
            } else if center < viewport[axis] * 2. / 3. {
                1
            } else {
                2
            }
        });
        let anchor = Self::ALL[usize::from(thirds[1] * 3 + thirds[0])];
        (anchor, anchor.offset_for_origin(at, size, viewport))
    }

    /// Inverts this specific anchor's reference point without choosing a new anchor.
    pub fn offset_for_origin(self, at: [f64; 2], size: [f64; 2], viewport: [f64; 2]) -> [f32; 2] {
        let fractions = self.fractions();
        std::array::from_fn(|axis| {
            let available = (viewport[axis] - size[axis]).max(0.);
            let direction = if fractions[axis] == 1. { -1. } else { 1. };
            ((at[axis] - available * fractions[axis]) * direction) as f32
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Covers every serialized anchor and its independent horizontal and vertical references.
    fn anchors() -> impl Iterator<Item = (Anchor, [f64; 2])> {
        [
            ("top_left", [0., 0.]),
            ("top_center", [0.5, 0.]),
            ("top_right", [1., 0.]),
            ("left", [0., 0.5]),
            ("center", [0.5, 0.5]),
            ("right", [1., 0.5]),
            ("bottom_left", [0., 1.]),
            ("bottom_center", [0.5, 1.]),
            ("bottom_right", [1., 1.]),
        ]
        .into_iter()
        .map(|(name, fractions)| {
            let anchor: Anchor = serde_json::from_value(serde_json::json!(name)).unwrap();
            assert_eq!(serde_json::to_value(anchor).unwrap(), name);
            (anchor, fractions)
        })
    }

    #[test]
    fn anchored_offsets_keep_all_nine_reference_distances_across_viewports_and_card_scales() {
        for (anchor, fractions) in anchors() {
            for scale in [0.5, 1., 2.] {
                let size = [100. * scale, 40. * scale];
                for viewport in [[640., 360.], [960., 540.]] {
                    let at = anchor.origin([13., 17.], size, viewport);
                    for axis in 0..2 {
                        let reference = viewport[axis] * fractions[axis];
                        let matching = at[axis] + size[axis] * fractions[axis];
                        let distance = if fractions[axis] == 1. {
                            reference - matching
                        } else {
                            matching - reference
                        };
                        assert!(
                            (distance - [13., 17.][axis]).abs() < 0.001,
                            "{anchor:?} {viewport:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn anchored_drop_in_each_viewport_third_reproduces_exact_origin() {
        let viewport = [900., 600.];
        let size = [100., 40.];
        for (expected, fractions) in anchors() {
            let at = std::array::from_fn(|axis| {
                viewport[axis] * (1. / 6. + fractions[axis] * 2. / 3.) - size[axis] * 0.5
            });
            let (anchor, offset) = Anchor::from_origin(at, size, viewport);
            assert_eq!(anchor, expected);
            let resolved = anchor.origin(offset, size, viewport);
            for axis in 0..2 {
                assert!((resolved[axis] - at[axis]).abs() < 0.001);
            }
        }
    }

    #[test]
    fn anchored_offsets_clamp_on_small_viewports_and_restore_on_larger_ones() {
        for (anchor, _) in anchors() {
            for offset in [[-1000., -1000.], [1000., 1000.]] {
                let at = anchor.origin(offset, [100., 40.], [120., 60.]);
                assert!(at.into_iter().all(|value| (0. ..=20.).contains(&value)));
                assert_eq!(anchor.origin(offset, [200., 100.], [120., 60.]), [0.; 2]);
            }
        }
        let offset = [23., 29.];
        assert_eq!(
            Anchor::BottomRight.origin(offset, [100., 40.], [110., 50.]),
            [0.; 2]
        );
        assert_eq!(
            Anchor::BottomRight.origin(offset, [100., 40.], [640., 360.]),
            [517., 291.]
        );
    }
}

#[cfg(test)]
mod inverse_tests {
    use super::*;
    #[test]
    fn explicit_anchor_inverse_does_not_change_with_viewport_thirds() {
        for anchor in Anchor::ALL {
            for viewport in [[455.333, 256.], [640., 360.], [427., 240.]] {
                let at = [37., 29.];
                let size = [32., 32.];
                let offset = anchor.offset_for_origin(at, size, viewport);
                let restored = anchor.origin(offset, size, viewport);
                assert!((0..2).all(|axis| (restored[axis] - at[axis]).abs() < 0.0001));
            }
        }
    }
    /// Fixed native previews retain measured origins, including partial and full overhang.
    #[test]
    fn unclamped_offsets_invert_measured_origins_without_changing_bounded_placement() {
        let viewport = [720., 1600.];
        for anchor in Anchor::ALL {
            for (at, size) in [
                ([-158., 613.], [533., 533.]),
                ([556., 530.], [220., 220.]),
                ([-20., -30.], [900., 1800.]),
            ] {
                let offset = anchor.offset_for_origin(at, size, viewport);
                let actual = anchor.origin_unclamped(offset, size, viewport);
                assert!((0..2).all(|axis| (actual[axis] - at[axis]).abs() < 0.001));
                let bounded = anchor.origin(offset, size, viewport);
                assert!((0..2).all(|axis| bounded[axis] >= 0.
                    && bounded[axis] + size[axis].min(viewport[axis]) <= viewport[axis]));
            }
        }
    }
}
