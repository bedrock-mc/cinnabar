//! Profile layout from the version-matched OreUI `p2`, `bZ`, `g2` and `h2` components.
//! Remaining parity gaps and exact references are recorded in docs/profile-parity.md.

mod achievements;
mod card;
mod rows;

use super::super::super::{IconRef, UiPresentationError};
use super::grid::{Grid, space};
use super::paint::{Bounds, Canvas};
use super::theme::{CAPTION, NEUTRAL80, SECONDARY_BUTTON, TEXT, TEXT_DIMMEST};
use super::widgets::{Variant, button, header, screen_overlay, tabs};
use crate::menu::ProfileTab;
use crate::menu::{MenuAction, MenuView, auth::AuthState};

/// Draws fixed navigation above independently scrolling card and active tab content.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    portrait: Option<IconRef>,
    artwork: &std::collections::HashMap<String, IconRef>,
) -> Result<(), UiPresentationError> {
    let [width, height] = size;
    screen_overlay(canvas, size)?;
    let top = header(
        canvas,
        view,
        "Your Profile",
        width,
        Some(MenuAction::AddBack),
    )? + space(canvas, 2);
    let bottom = height - space(canvas, 2);
    let grid = Grid::new(canvas.r(1.0), width);
    if view.auth_state != AuthState::Authenticated {
        let [left, right] = grid.span(
            if grid.narrow { 0 } else { 1 },
            if grid.narrow { 8 } else { 10 },
        );
        return error(canvas, view, [left, top, right, bottom], true);
    }
    if view.feeds.profile.unavailable {
        let [left, right] = grid.span(
            if grid.narrow { 0 } else { 1 },
            if grid.narrow { 8 } else { 10 },
        );
        return error(canvas, view, [left, top, right, bottom], false);
    }
    if !view.feeds.profile.loaded {
        return loading(canvas, [0.0, top, width, bottom]);
    }
    let avatar = artwork
        .get(&view.feeds.profile.avatar_path)
        .copied()
        .filter(|_| !view.feeds.profile.avatar_error);
    let featured = artwork
        .get(&view.feeds.profile.featured_screenshot_path)
        .copied()
        .filter(|_| !view.feeds.profile.featured_screenshot_error);
    let [left, right] = grid.span(if grid.narrow { 0 } else { 4 }, 8);
    if !grid.narrow {
        let [card_left, card_right] = grid.span(0, 4);
        let waiting =
            !view.feeds.profile.avatar_loaded || !view.feeds.profile.featured_screenshot_loaded;
        if waiting {
            loading(canvas, [card_left, top, card_right, bottom])?;
        } else {
            let scroll =
                canvas.begin_scroll("profile_card", [card_left, top, card_right, bottom])?;
            let offset = scroll.offset;
            let end = card::draw(
                canvas,
                view,
                [card_left, top - scroll.offset, card_right, bottom],
                portrait,
                avatar,
                featured,
                false,
            )?;
            canvas.end_scroll(scroll, end - top + offset)?;
        }
    }
    let tab_bottom = top + canvas.r(4.8);
    tabs(
        canvas,
        view,
        [left, top, right, tab_bottom],
        &[
            (
                "Overview",
                Some(MenuAction::SelectProfileTab(ProfileTab::Overview)),
            ),
            (
                "Stats",
                Some(MenuAction::SelectProfileTab(ProfileTab::Stats)),
            ),
        ],
        usize::from(view.profile_tab == ProfileTab::Stats),
    )?;
    let content_top = tab_bottom + space(canvas, 2);
    let scroll = canvas.begin_scroll("profile_body", [left, content_top, right, bottom])?;
    let offset = scroll.offset;
    let mut y = content_top - offset;
    if grid.narrow {
        y = card::draw(
            canvas,
            view,
            [left, y, right, bottom],
            portrait,
            avatar,
            featured,
            true,
        )? + space(canvas, 2);
    }
    y = match view.profile_tab {
        ProfileTab::Overview => {
            let end = rows::overview(canvas, view, [left, y, right, bottom])?;
            achievements::draw(
                canvas,
                view,
                [left, end + space(canvas, 2), right, bottom],
                artwork,
            )?
        }
        ProfileTab::Stats => rows::statistics(canvas, view, [left, y, right, bottom])?,
    };
    canvas.end_scroll(scroll, y + offset - content_top + space(canvas, 2))
}

/// Shows the route-wide signed-out or offline state; unavailable services never become zeros.
fn error(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    signed_out: bool,
) -> Result<(), UiPresentationError> {
    let (title, text) = if signed_out {
        (
            "Profile Unavailable",
            "Please sign in to a Microsoft account to view player profiles.",
        )
    } else {
        (
            "Profile couldn't load!",
            "We encountered an unknown error, please try again.",
        )
    };
    let scroll = canvas.begin_scroll("profile_error", b)?;
    let offset = scroll.offset;
    let pad = canvas.r(1.6);
    let width = (b[2] - b[0] - pad * 2.0).max(1.0);
    let text_height = canvas.measure_height(text, width, CAPTION)?;
    let art_width = canvas
        .r(if width >= canvas.r(51.2) { 51.2 } else { 25.6 })
        .min(width);
    let art_height = art_width * 96.0 / 256.0;
    let height = pad * 2.0
        + canvas.r(SECONDARY_BUTTON.line)
        + space(canvas, 4) * 2.0
        + art_height
        + text_height
        + space(canvas, 2)
        + canvas.r(4.4);
    let top = b[1] - offset;
    super::widgets::panel(canvas, [b[0], top, b[2], top + height])?;
    let mut y = top + pad;
    y += canvas.centered_wrapped_text(title, [b[0] + pad, y], width, SECONDARY_BUTTON, TEXT)?
        + space(canvas, 4);
    let art_left = (b[0] + b[2] - art_width) * 0.5;
    let image = crate::ui_runtime::oreui_assets::PROFILE_ERRORS[usize::from(!signed_out)];
    let _ = canvas.sprite(
        image,
        [art_left, y, art_left + art_width, y + art_height],
        [255; 4],
    )?;
    y += art_height + space(canvas, 4);
    y += canvas.centered_wrapped_text(text, [b[0] + pad, y], width, CAPTION, TEXT_DIMMEST)?
        + space(canvas, 2);
    let button_width = (width * 0.7).min(canvas.r(32.0));
    let button_left = (b[0] + b[2] - button_width) * 0.5;
    button(
        canvas,
        view,
        [
            button_left,
            y,
            button_left + button_width,
            y + canvas.r(4.4),
        ],
        if signed_out {
            Variant::Primary
        } else {
            Variant::Secondary
        },
        if signed_out {
            "Sign in with Microsoft"
        } else {
            "Try again"
        },
        Some(if signed_out {
            MenuAction::StartSignIn
        } else {
            MenuAction::RefreshProfile
        }),
    )?;
    canvas.end_scroll(scroll, height)
}

/// Draws the reference's two-rem animation while the service is pending.
fn loading(canvas: &mut Canvas<'_>, b: Bounds) -> Result<(), UiPresentationError> {
    let side = canvas.r(2.0);
    let x = (b[0] + b[2] - side) * 0.5;
    let y = b[1] + canvas.r(6.4);
    if canvas.loading_sprite([x, y, x + side, y + side])? {
        return Ok(());
    }
    canvas.frame([x, y, x + side, y + side], 0.4, NEUTRAL80.hovered)?;
    canvas.fill([x, y, x + side, y + canvas.r(0.4)], TEXT)
}
