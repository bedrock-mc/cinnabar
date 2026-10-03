//! The profile route (`/profile/overview`): header with back, the player card
//! in four of twelve columns (16:9 banner, large gamerpic, name, status, the
//! primary action) and the Overview/Stats tabs with their rows in eight.

use super::super::super::{IconRef, UiPresentationError};
use super::grid::{Grid, space};
use super::icons::{self, Icon};
use super::paint::Canvas;
use super::theme::{BODY, CAPTION, HEADER5, NEUTRAL100, TEXT, TEXT_DIMMER};
use super::widgets::{Variant, button, header, panel, row, screen_overlay, tabs};
use crate::menu::{MenuAction, MenuScreen, MenuView, auth::AuthState};

/// Large gamerpic side, in rem.
const GAMERPIC: f32 = 9.6;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    portrait: Option<IconRef>,
) -> Result<(), UiPresentationError> {
    let [width, height] = size;
    screen_overlay(canvas, size)?;
    let mut top = header(
        canvas,
        view,
        "Profile",
        width,
        Some(MenuAction::Navigate(MenuScreen::Home)),
    )? + space(canvas, 2);
    let grid = Grid::new(canvas.r(1.0), width);
    let (card_span, content_span) = if grid.narrow {
        ((0, 8), (0, 8))
    } else {
        ((0, 4), (4, 8))
    };
    let [left, right] = grid.span(card_span.0, card_span.1);
    let pad = space(canvas, 4);
    let pic = canvas.r(GAMERPIC);
    let text_left = left + pad;
    let text_width = right - left - pad * 2.0;
    let profile = &view.feeds.profile;
    let name = if profile.gamertag.is_empty() {
        view.display_name.as_str()
    } else {
        profile.gamertag.as_str()
    };
    let status = [profile.real_name.as_str(), profile.presence.as_str()]
        .into_iter()
        .find(|text| !text.is_empty())
        .unwrap_or(if view.auth_state == AuthState::Authenticated {
            "Online"
        } else {
            "Offline"
        });
    let bottom = height - space(canvas, 2);
    let original_top = top;
    let available = (bottom - top).max(0.0);
    let span = grid.span(0, if grid.narrow { 8 } else { 12 });
    let scroll = canvas.begin_scroll("profile_body", [span[0], top, span[1], bottom])?;
    top -= scroll.offset;
    let intrinsic = (right - left) * 9.0 / 16.0
        + pic * 0.5
        + space(canvas, 2)
        + canvas.measure_height(name, text_width, HEADER5)?
        + space(canvas, 1)
        + canvas.measure_height(status, text_width, CAPTION)?
        + space(canvas, 2)
        + canvas.r(4.4)
        + pad;
    let card_bottom = top + intrinsic.max(if grid.narrow { 0.0 } else { available });
    panel(canvas, [left, top, right, card_bottom])?;
    let banner_bottom = top + (right - left) * 9.0 / 16.0;
    canvas.fill(
        [
            left + canvas.r(0.2),
            top + canvas.r(0.2),
            right - canvas.r(0.2),
            banner_bottom,
        ],
        NEUTRAL100,
    )?;
    let pic_bounds = [
        left + pad,
        banner_bottom - pic * 0.5,
        left + pad + pic,
        banner_bottom + pic * 0.5,
    ];
    match portrait {
        Some(icon) => {
            canvas.fill(pic_bounds, [0x1e, 0x1e, 0x1f, 255])?;
            canvas.icon_ref(icon, pic_bounds)?;
        }
        None => {
            canvas.fill(pic_bounds, [0x48, 0x49, 0x4a, 255])?;
            let [w, h] = Icon::Player.texels();
            let texel = canvas.r(0.2);
            let at = [
                (pic_bounds[0] + pic_bounds[2] - w as f32 * texel) * 0.5,
                (pic_bounds[1] + pic_bounds[3] - h as f32 * texel) * 0.5,
            ];
            icons::draw(canvas, Icon::Player, at, TEXT_DIMMER)?;
        }
    }
    canvas.frame(pic_bounds, 0.2, [0x1e, 0x1e, 0x1f, 255])?;
    let mut y = pic_bounds[3] + space(canvas, 2);
    y += canvas.text(name, [text_left, y], text_width, HEADER5, TEXT, false)? + space(canvas, 1);
    y += canvas.text(
        status,
        [text_left, y],
        text_width,
        CAPTION,
        TEXT_DIMMER,
        false,
    )? + space(canvas, 2);
    let (label, action) = if view.auth_state == AuthState::Authenticated {
        ("Dressing Room", None)
    } else {
        ("Sign In", Some(MenuAction::StartSignIn))
    };
    button(
        canvas,
        view,
        [text_left, y, text_left + text_width, y + canvas.r(4.4)],
        Variant::Primary,
        label,
        action,
    )?;

    let [content_left, content_right] = grid.span(content_span.0, content_span.1);
    let content_top = if grid.narrow {
        card_bottom + space(canvas, 2)
    } else {
        top
    };
    let tab_bottom = content_top + canvas.r(5.2);
    tabs(
        canvas,
        view,
        [content_left, content_top, content_right, tab_bottom],
        &[("Overview", None), ("Stats", None)],
        0,
    )?;
    // An unavailable count stays blank rather than reading as zero.
    let count = |value: Option<i64>| value.map_or_else(String::new, |n| n.to_string());
    let rows = [
        ("Friends", count(profile.friends.map(i64::from))),
        ("Followers", count(profile.followers.map(i64::from))),
        ("Gamerscore", count(profile.gamerscore)),
    ];
    let mut row_top = tab_bottom + space(canvas, 2);
    let row_height = canvas.r(6.4);
    for (label, value) in rows {
        let bounds = [content_left, row_top, content_right, row_top + row_height];
        row(canvas, view, bounds, false, None)?;
        let inner = canvas.r(2.4);
        let text_top = row_top + (row_height - canvas.r(BODY.line)) * 0.5;
        canvas.text(
            label,
            [bounds[0] + inner, text_top],
            (bounds[2] - bounds[0]) * 0.6,
            BODY,
            TEXT,
            false,
        )?;
        let value_width = canvas.measure(&value, BODY)?;
        canvas.text(
            &value,
            [bounds[2] - inner - value_width, text_top],
            value_width + 1.0,
            BODY,
            TEXT_DIMMER,
            false,
        )?;
        row_top += row_height + space(canvas, 1);
    }
    let content = row_top.max(card_bottom) + scroll.offset - original_top;
    canvas.end_scroll(scroll, content)
}
