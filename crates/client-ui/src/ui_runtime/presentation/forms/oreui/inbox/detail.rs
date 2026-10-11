//! The selected message has its own scrollable view, as in OreUI's message route.
use super::super::grid::Grid;
use super::super::motion::Surface;
use super::super::theme::{BORDER, EDGE, NEUTRAL80};
use {
    super::{BODY, Canvas, TEXT, TEXT_DIMMER, UiPresentationError, header, screen_overlay},
    launcher::menu::{InboxItem, MenuAction, MenuView, inbox::Action},
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
    use std::hash::{Hash, Hasher};
    let mut identity = std::collections::hash_map::DefaultHasher::new();
    item.instance_id.hash(&mut identity);
    let entrance = canvas.begin_entrance(Surface::InboxMessage(identity.finish()));
    let grid = Grid::with_breakpoint(canvas.rem, size[0], 102.0);
    let span = if grid.narrow {
        grid.span(1, 6)
    } else {
        grid.span(1, 10)
    };
    let pad = canvas.r(1.6);
    let panel = [
        span[0],
        top + canvas.r(0.8),
        span[1],
        size[1] - canvas.r(0.8),
    ];
    canvas.fill(panel, NEUTRAL80.fill)?;
    canvas.frame(panel, EDGE, BORDER)?;
    let bounds = [
        panel[0] + pad,
        panel[1] + pad,
        panel[2] - pad,
        panel[3] - pad,
    ];
    let scroll = canvas.begin_scroll("inbox_message", bounds)?;
    let start = bounds[1] - scroll.offset;
    let width = bounds[2] - bounds[0];
    let mut y = start;
    y += canvas.text(&item.header, [bounds[0], y], width, BODY, TEXT, false)? + pad;
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
        [bounds[0], y],
        width,
        BODY,
        TEXT_DIMMER,
        false,
    )? + pad;
    y += canvas.text(&item.body, [bounds[0], y], width, BODY, TEXT, false)?;
    canvas.end_scroll(scroll, y - start)?;
    canvas.end_entrance(entrance, size)
}
