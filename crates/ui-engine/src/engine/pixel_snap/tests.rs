use super::{positioned, snapped};

/// Whether every edge of `rect` lies on a whole physical pixel at `dpi` physical pixels per
/// logical pixel.
fn whole(rect: [f32; 4], dpi: f32) -> bool {
    rect.iter()
        .all(|edge| ((edge * dpi) - (edge * dpi).round()).abs() < 1e-3)
}

/// A 195×152 terminal centred in a 480×271 GUI-unit screen sits at (142.5, 59.5); at GUI
/// scale 3 that is (427.5, 178.5) physical pixels, which vanilla truncates to (427, 178).
#[test]
fn a_control_centred_in_odd_free_space_lands_on_whole_pixels() {
    let rect = snapped([142.5, 59.5, 195.0, 152.0], 3.0, 3.0);
    assert_eq!(rect, [427.0, 178.0, 1012.0, 634.0]);
    assert!(whole(rect, 1.0));
}

/// Sizes round up to whole pixels from the truncated position, as vanilla adds
/// `ceil(w * s)` to `(int)(x * s)`.
#[test]
fn sizes_round_up_to_whole_pixels() {
    assert_eq!(
        snapped([0.5, 0.0, 0.5, 1.0], 3.0, 3.0),
        [1.0, 0.0, 3.0, 3.0]
    );
    assert_eq!(snapped([0.0, 0.0, 1.0 / 3.0, 1.0], 2.0, 2.0)[2], 1.0);
}

/// Positions truncate toward zero, as the `(int)` cast does, so a control left of the
/// origin moves right.
#[test]
fn negative_positions_truncate_toward_zero() {
    assert_eq!(
        snapped([-1.5, 0.0, 2.0, 2.0], 3.0, 3.0),
        [-4.0, 0.0, 2.0, 6.0]
    );
}

/// The snap is in physical pixels: at DPI 1.25 and GUI scale 3 (2.4 logical pixels per
/// unit) the logical edges are fractional and the physical ones whole.
#[test]
fn edges_are_whole_in_physical_pixels_at_any_dpi() {
    for (x, y) in [(142.5, 59.5), (7.0, 17.0), (0.25, 100.75), (333.3, 12.1)] {
        let rect = snapped([x, y, 18.0, 20.0], 3.0, 2.4);
        assert!(whole(rect, 1.25), "{x},{y}: {rect:?}");
    }
}

/// A position already whole in physical pixels stays put despite float noise.
#[test]
fn whole_positions_survive_float_noise() {
    assert_eq!(snapped([0.999_999_9, 0.0, 1.0, 1.0], 3.0, 3.0)[0], 3.0);
    assert_eq!(snapped([0.0, 0.0, 1.000_000_1, 1.0], 3.0, 3.0)[2], 3.0);
}

/// Adjacent slices of one nine-slice never open a gap between them.
#[test]
fn adjacent_slices_stay_joined() {
    for x in [142.5, 142.25, 0.75, 7.0] {
        for split in [0.5, 1.0, 1.5, 4.25] {
            let left = snapped([x, 0.0, split, 1.0], 3.0, 3.0);
            let right = snapped([x + split, 0.0, 6.0, 1.0], 3.0, 3.0);
            assert!(left[2] >= right[0], "{x}+{split}: {left:?} {right:?}");
        }
    }
}

/// Text moves to the pixel grid but keeps its size, so its wrap width is unchanged.
#[test]
fn text_keeps_its_size() {
    assert_eq!(
        positioned([142.5, 59.5, 10.25, 9.0], 3.0, 3.0),
        [427.0, 178.0, 457.75, 205.0]
    );
}

/// Nearest sampling inside the snapped quad stays inside its uv rect with margin: an 18×20
/// toolbar button background (rows 128..148 of its sheet, an opaque grid row at 127) at GUI
/// scale 3, on the half unit the live client drew it at. Unsnapped, the first pixel centre
/// samples exactly the texel edge at 128, where interpolation error picks row 127.
#[test]
fn nearest_sampling_stays_inside_the_uv_rect() {
    let [_, y0, _, y1] = snapped([127.5, 62.5, 18.0, 20.0], 3.0, 3.0);
    let (v0, v1) = (128.0_f32, 148.0_f32);
    // Rasterization covers the pixel rows whose centres lie in [y0, y1).
    let first = (y0 - 0.5).ceil() as i32;
    let last = (y1 - 0.5).ceil() as i32 - 1;
    assert!(first <= last);
    for row in first..=last {
        let centre = row as f32 + 0.5;
        let v = v0 + (centre - y0) / (y1 - y0) * (v1 - v0);
        assert!((v0..v1).contains(&v.floor()), "row {row} samples {v}");
        assert!(
            (v - v.round()).abs() > 0.1,
            "row {row} samples {v}, on a texel edge"
        );
    }
}
