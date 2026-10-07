//! Inbox maintenance and deletion use the same immediate inputs and animated surfaces.

use super::super::grid::Grid;
use super::super::motion::Surface;
use super::super::theme::TEXT;
use super::super::widgets::{Variant, button};
use super::{Action, Canvas, MenuAction, MenuView, UiPresentationError, header, screen_overlay};

pub(super) fn settings(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    screen_overlay(canvas, size)?;
    let top = header(
        canvas,
        view,
        "INBOX SETTINGS",
        size[0],
        Some(MenuAction::Inbox(Action::Filters)),
    )? + canvas.r(1.6);
    let grid = Grid::with_breakpoint(canvas.rem, size[0], 102.0);
    let span = if grid.narrow {
        grid.span(1, 6)
    } else {
        grid.span(1, 10)
    };
    let entrance = canvas.begin_entrance(Surface::InboxSettings);
    let scroll = canvas.begin_scroll("oreui_inbox_settings", [span[0], top, span[1], size[1]])?;
    let mut y = top - scroll.offset;
    for (label, variant, action) in [
        ("Mark all as read", Variant::Secondary, Action::MarkAllRead),
        (
            "Delete all read messages",
            Variant::Destructive,
            Action::DeleteAllRead,
        ),
    ] {
        button(
            canvas,
            view,
            [span[0], y, span[1], y + canvas.r(4.4)],
            variant,
            label,
            Some(MenuAction::Inbox(action)),
        )?;
        y += canvas.r(5.2);
    }
    let content = y + scroll.offset - top;
    canvas.end_scroll(scroll, content)?;
    canvas.end_entrance(entrance, size)?;
    delete_dialog(canvas, view, size)
}

pub(super) fn delete_dialog(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    let Some(pending) = &view.feeds.inbox_state.delete_pending else {
        return Ok(());
    };
    canvas.clear_focus_geometry();
    canvas.scrolls.clear();
    let dialog = super::super::modal::Modal {
        title: if pending.len() > 1 {
            "Delete read messages?"
        } else {
            "Delete message?"
        },
        items: Vec::new(),
        body: "This cannot be undone.".into(),
        body_color: TEXT,
        buttons: vec![
            (
                "Delete".into(),
                Variant::Destructive,
                Some(MenuAction::Inbox(Action::ConfirmDelete)),
            ),
            (
                "Cancel".into(),
                Variant::Secondary,
                Some(MenuAction::Inbox(Action::Cancel)),
            ),
        ],
        close: Some(MenuAction::Inbox(Action::Cancel)),
    };
    super::super::modal::draw(canvas, view, size, &dialog)?;
    Ok(())
}
