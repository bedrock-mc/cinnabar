//! The selected message has its own scrollable view, as in OreUI's message route.
use super::{
    Action, BODY, Canvas, InboxItem, MenuAction, MenuView, TEXT, TEXT_DIMMER, UiPresentationError,
    header, screen_overlay,
};

/// Shows the selected message without expanding its summary into the list.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    item: &InboxItem,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    screen_overlay(canvas, size)?;
    let top = header(
        canvas,
        view,
        "INBOX",
        size[0],
        Some(MenuAction::Inbox(Action::Cancel)),
    )?;
    let pad = canvas.r(3.2);
    let bounds = [pad, top + pad, size[0] - pad, size[1] - pad];
    let scroll = canvas.begin_scroll("inbox_message", bounds)?;
    let start = bounds[1] - scroll.offset;
    let width = bounds[2] - bounds[0];
    let mut y = start;
    y += canvas.text(&item.header, [pad, y], width, BODY, TEXT, false)? + pad;
    y += canvas.text(
        &format!(
            "{}   {}",
            if item.source.is_empty() {
                "Minecraft"
            } else {
                &item.source
            },
            super::inbox::date(&item.received)
        ),
        [pad, y],
        width,
        BODY,
        TEXT_DIMMER,
        false,
    )? + pad;
    y += canvas.text(&item.body, [pad, y], width, BODY, TEXT, false)?;
    canvas.end_scroll(scroll, y - start)
}
