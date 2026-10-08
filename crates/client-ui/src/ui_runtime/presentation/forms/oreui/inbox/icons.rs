//! Inbox glyphs and runtime category artwork.
use super::super::motion::{Kind, opacity};
use super::super::theme::{EDGE, NEUTRAL20};
use super::{Action, Canvas, MenuAction, MenuView, TEXT, UiPresentationError};

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
    super::super::icons::draw(
        canvas,
        super::super::icons::Icon::Filter,
        at,
        NEUTRAL20.text,
    )
}

/// Category tiles remain distinct even without the optional OreUI atlas.
pub(super) fn category_icon(
    canvas: &mut Canvas<'_>,
    index: usize,
    at: [f32; 2],
) -> Result<(), UiPresentationError> {
    let side = super::super::icons::native_side(canvas);
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
    let s = side;
    let [x, y] = at;
    canvas.fill([x, y, x + s, y + s], [30, 30, 31, 255])?;
    let inset = canvas.r(0.4);
    canvas.fill(
        [x + inset, y + inset, x + s - inset, y + s - inset],
        colors[index],
    )
}

pub(super) fn filter_button(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    width: f32,
) -> Result<(), UiPresentationError> {
    let edge = canvas.r(EDGE);
    let bounds = [
        width - canvas.r(4.0) - edge,
        edge,
        width - edge,
        canvas.r(4.4) - edge,
    ];
    let action = MenuAction::Inbox(Action::Filters);
    let state = canvas.interaction(view, Some(action));
    let motion = canvas.feedback(state, true, false, Kind::Surface);
    let role = canvas.role(NEUTRAL20);
    canvas.fill(bounds, motion.color(role.fill, role.hovered, role.pressed))?;
    if motion.focus > 0.0 {
        canvas.frame(bounds, EDGE, opacity(role.text, motion.focus))?;
    }
    filter_icon(
        canvas,
        [
            (bounds[0] + bounds[2] - canvas.r(2.4)) * 0.5,
            (bounds[1] + bounds[3] - canvas.r(2.4)) * 0.5,
        ],
    )?;
    canvas.hit(action, bounds)
}
