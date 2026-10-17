//! Equal, quiet utility actions beneath the home panels.

use super::*;

/// Draws the three utility actions with equal widths and no account controls.
pub(super) fn draw(canvas: &mut Canvas<'_>, view: &MenuView, b: Bounds) -> Result<(), PaintError> {
    let region = canvas.begin_focus_region(focus::HEADER, b, None, true)?;
    let gap = canvas.r(1.2);
    let friends = if view.friends.is_empty() {
        "Friends".to_owned()
    } else {
        format!("Friends ({})", view.friends.len())
    };
    let unread = view.feeds.home.inbox_unread;
    let inbox = if unread == 0 {
        "Inbox".to_owned()
    } else {
        format!(
            "Inbox ({}{})",
            unread.min(99),
            if unread > 99 { "+" } else { "" }
        )
    };
    let actions = [
        (friends.as_str(), MenuAction::Navigate(MenuScreen::Friends)),
        (inbox.as_str(), MenuAction::Navigate(MenuScreen::Inbox)),
        ("Quit", MenuAction::OpenExitDialog),
    ];
    let width = (b[2] - b[0] - gap * (actions.len() - 1) as f32) / actions.len() as f32;
    for (index, (label, action)) in actions.into_iter().enumerate() {
        let left = b[0] + (width + gap) * index as f32;
        widgets::button(
            canvas,
            view,
            [left, b[1], left + width, b[3]],
            Variant::Neutral,
            label,
            Some(action),
        )?;
    }
    canvas.end_focus_region(region);
    Ok(())
}
