//! The vanilla empty state for unowned world templates.

use super::*;

/// "Owned by me" with nothing owned: vanilla's no-content message and the Marketplace way out.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    area: Bounds,
) -> Result<(), UiPresentationError> {
    let card = [area[0], area[1], area[2], area[1] + canvas.r(22.0)];
    panel(canvas, card)?;
    let pad = space(canvas, 4);
    let inner = [card[0] + pad, card[1] + pad, card[2] - pad, card[3] - pad];
    canvas.text_centred(
        "You don\u{2019}t own any content... yet!",
        [inner[0], inner[1], inner[2], inner[1] + canvas.r(2.4)],
        SECONDARY_BUTTON,
        TEXT,
        false,
    )?;
    let body = "Explore thousands of original worlds, templates, and skins on Marketplace \u{2014} or import your own.";
    canvas.text(
        body,
        [inner[0], inner[1] + canvas.r(3.6)],
        inner[2] - inner[0],
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    let width = canvas.r(24.0).min(inner[2] - inner[0]);
    let left = (inner[0] + inner[2] - width) * 0.5;
    button(
        canvas,
        view,
        [left, inner[3] - canvas.r(CONTROL), left + width, inner[3]],
        Variant::Primary,
        "Go to Marketplace",
        Some(MenuAction::Store(crate::store::OPEN)),
    )
}
