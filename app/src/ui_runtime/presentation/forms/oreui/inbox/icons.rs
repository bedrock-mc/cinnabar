//! Inbox glyphs and runtime category artwork.
use super::{Canvas, TEXT, UiPresentationError};

/// Pixel outline of the trash glyph in the supplied vanilla capture.
pub(super) fn trash_icon(canvas: &mut Canvas<'_>, at: [f32; 2]) -> Result<(), UiPresentationError> {
    let [x, y] = at;
    let p = canvas.r(0.2);
    for [a, b, c, d] in [
        [0., 3., 9., 4.],
        [2., 1., 7., 2.],
        [2., 2., 3., 3.],
        [6., 2., 7., 3.],
        [1., 5., 2., 13.],
        [7., 5., 8., 13.],
        [2., 12., 7., 13.],
        [3., 6., 4., 11.],
        [5., 6., 6., 11.],
    ] {
        canvas.fill([x + a * p, y + b * p, x + c * p, y + d * p], TEXT)?;
    }
    Ok(())
}

/// Three filter sliders, as in the supplied header capture.
pub(super) fn filter_icon(
    canvas: &mut Canvas<'_>,
    at: [f32; 2],
) -> Result<(), UiPresentationError> {
    let [x, y] = at;
    let p = canvas.r(0.2);
    for (row, knob) in [(1., 6.), (5., 2.), (9., 7.)] {
        canvas.fill(
            [x, y + row * p, x + 11. * p, y + (row + 1.) * p],
            [30, 30, 31, 255],
        )?;
        canvas.fill(
            [
                x + knob * p,
                y + (row - 1.) * p,
                x + (knob + 2.) * p,
                y + (row + 2.) * p,
            ],
            [30, 30, 31, 255],
        )?;
    }
    Ok(())
}

/// Category tiles remain distinct even without the optional OreUI atlas.
pub(super) fn category_icon(
    canvas: &mut Canvas<'_>,
    index: usize,
    at: [f32; 2],
) -> Result<(), UiPresentationError> {
    let side = canvas.r(2.4);
    if canvas.sprite(
        crate::ui_runtime::oreui_assets::INBOX_ICONS[index],
        [at[0], at[1], at[0] + side, at[1] + side],
        [255; 4],
    )? {
        return Ok(());
    }
    let colors = [
        [184, 132, 66, 255],
        [129, 49, 210, 255],
        [241, 173, 84, 255],
        [236, 195, 47, 255],
        [156, 83, 54, 255],
    ];
    let s = canvas.r(2.2);
    let [x, y] = at;
    canvas.fill([x, y, x + s, y + s], [30, 30, 31, 255])?;
    let inset = canvas.r(0.4);
    canvas.fill(
        [x + inset, y + inset, x + s - inset, y + s - inset],
        colors[index],
    )
}
