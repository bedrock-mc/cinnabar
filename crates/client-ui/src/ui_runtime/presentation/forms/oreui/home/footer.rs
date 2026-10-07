use super::super::icons::{self, Icon};
use super::super::theme::SECONDARY_BUTTON;
use super::*;
use crate::menu::auth::AuthState;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    compact: bool,
    portrait: Option<IconRef>,
) -> Result<(), UiPresentationError> {
    let region = canvas.begin_focus_region(focus::HEADER, b, None, true)?;
    let gap = canvas.r(1.2);
    let height = canvas.r(4.8);
    let widths = [20.0, 14.0, 16.0, 8.0];
    let total = widths.iter().sum::<f32>();
    let gaps = gap * (widths.len() - 1) as f32;
    let row_width = (canvas.r(total) + gaps).min(b[2] - b[0]);
    let left = (b[0] + b[2] - row_width) * 0.5;
    let mut x = left;
    let mut bounds = [[0.0; 4]; 4];
    for (i, width) in widths.into_iter().enumerate() {
        bounds[i] = if compact {
            let width = (b[2] - b[0] - gap) * 0.5;
            let x = b[0] + (width + gap) * (i % 2) as f32;
            let y = b[1] + (height + gap) * (i / 2) as f32;
            [x, y, x + width, y + height]
        } else {
            let width = (row_width - gaps) * width / total;
            let bound = [x, b[1], x + width, b[1] + height];
            x += width + gap;
            bound
        };
    }
    account(canvas, view, bounds[0], portrait)?;
    let friends = if view.friends.is_empty() {
        "Friends".to_owned()
    } else {
        format!("Friends ({})", view.friends.len())
    };
    let inbox = if view.feeds.home.inbox_unread == 0 {
        "Inbox".to_owned()
    } else {
        let unread = view.feeds.home.inbox_unread;
        format!(
            "Inbox ({}{})",
            unread.min(99),
            if unread > 99 { "+" } else { "" }
        )
    };
    for (i, (label, action)) in [
        (friends.as_str(), MenuAction::Navigate(MenuScreen::Friends)),
        (inbox.as_str(), MenuAction::Navigate(MenuScreen::Inbox)),
        ("Quit", MenuAction::OpenExitDialog),
    ]
    .into_iter()
    .enumerate()
    {
        widgets::button(
            canvas,
            view,
            bounds[i + 1],
            Variant::Neutral,
            label,
            Some(action),
        )?;
    }
    canvas.end_focus_region(region);
    Ok(())
}

fn account(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    portrait: Option<IconRef>,
) -> Result<(), UiPresentationError> {
    let action = match view.auth_state {
        AuthState::SignedOut | AuthState::Failed(_) => MenuAction::StartSignIn,
        _ => MenuAction::OpenAccounts,
    };
    let state = canvas.interaction(view, Some(action));
    widgets::button_face(canvas, b, Variant::Neutral, "", state, true)?;
    let motion = canvas.feedback(state, true, false, Kind::Button);
    let top = b[1] + canvas.r(0.4) * motion.press;
    let bottom = b[3] - canvas.r(0.4) * (1.0 - motion.press);
    let label = match view.auth_state {
        AuthState::SignedOut | AuthState::Failed(_) => "Sign in",
        AuthState::Checking => "Signing in...",
        _ => "Account",
    };
    let size = canvas.r(2.4);
    let gap = canvas.r(0.8);
    let label_width = canvas.measure(label, SECONDARY_BUTTON)?;
    let x = (b[0] + b[2] - size - gap - label_width) * 0.5;
    let y = (top + bottom - size) * 0.5;
    if let Some(icon) = super::super::super::accounts::current_picture(view)
        .and_then(|path| canvas.artwork?.get(path).copied())
        .or(portrait)
    {
        canvas.icon_ref(icon, [x, y, x + size, y + size])?;
    } else {
        let [w, h] = Icon::Player
            .texels()
            .map(|value| canvas.r(EDGE) * value as f32);
        icons::draw(
            canvas,
            Icon::Player,
            [x + (size - w) * 0.5, y + (size - h) * 0.5],
            TEXT_DIMMER,
        )?;
    }
    let left = x + size + gap;
    canvas.text_line_vertically_centred(
        label,
        [left, top, b[2] - canvas.r(0.8), bottom],
        SECONDARY_BUTTON,
        TEXT,
    )?;
    canvas.hit(action, b)
}
