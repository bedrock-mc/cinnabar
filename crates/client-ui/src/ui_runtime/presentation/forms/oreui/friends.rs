//! The friends drawer (`/friends-drawer`): a 37.6rem drawer on the right with
//! the search field and close button, the People / Party / World tabs, and the
//! friends playing now (each row joins that friend's world).

use super::super::super::UiPresentationError;
use super::grid::space;
use super::icons::{self, Icon};
use super::paint::Canvas;
use super::theme::{BODY, BORDER, CAPTION, NEUTRAL90, TEXT, TEXT_DIMMER, TEXT_DIMMEST};
use super::widgets::{Variant, button, panel, row, screen_overlay, tabs};
use launcher::menu::{MenuAction, MenuScreen, MenuView};

const DRAWER_WIDTH: f32 = 37.6;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    let [width, height] = size;
    screen_overlay(canvas, size)?;
    let drawer_width = canvas.r(DRAWER_WIDTH).min(width);
    let drawer = [width - drawer_width, 0.0, width, height];
    panel(canvas, drawer)?;
    let pad = space(canvas, 2);
    let left = drawer[0] + pad;
    let right = drawer[2] - pad;
    let top = pad;

    // Search field and close button.
    let close_width = canvas.r(4.8);
    let field_bottom = top + canvas.r(4.4);
    let field = [left, top, right - close_width - pad, field_bottom];
    canvas.fill(field, NEUTRAL90)?;
    canvas.frame(field, 0.2, BORDER)?;
    let glyph = canvas.r(0.2);
    icons::draw(
        canvas,
        Icon::Search,
        [field[0] + canvas.r(1.2), top + canvas.r(1.0)],
        TEXT_DIMMEST,
    )?;
    canvas.text(
        "Search for people",
        [field[0] + canvas.r(1.2) + 13.0 * glyph, top + canvas.r(1.2)],
        field[2] - field[0] - canvas.r(4.0),
        BODY,
        TEXT_DIMMEST,
        false,
    )?;
    button(
        canvas,
        view,
        [right - close_width, top, right, field_bottom],
        Variant::Neutral,
        "",
        Some(MenuAction::Navigate(MenuScreen::Home)),
    )?;
    let [cross_w, cross_h] = Icon::Cross.texels();
    icons::draw(
        canvas,
        Icon::Cross,
        [
            right - close_width * 0.5 - cross_w as f32 * glyph * 0.5,
            top + (canvas.r(4.0) - cross_h as f32 * glyph) * 0.5,
        ],
        TEXT,
    )?;

    let tab_top = field_bottom + pad;
    let tab_bottom = tab_top + canvas.r(5.2);
    tabs(
        canvas,
        view,
        [left, tab_top, right, tab_bottom],
        &[("People", None), ("Party", None), ("World", None)],
        0,
    )?;

    let mut y = tab_bottom + pad;
    if view.friends.is_empty() {
        canvas.text_centred(
            "None of your friends are playing right now.",
            [left, y, right, y + canvas.r(4.0)],
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
        return Ok(());
    }
    let list_top = y;
    let scroll = canvas.begin_scroll("friends_list", [left, list_top, right, height - pad])?;
    y -= scroll.offset;
    let row_height = canvas.r(6.4);
    for (index, friend) in view.friends.iter().enumerate() {
        if y + row_height < list_top || y > height - pad {
            y += row_height + space(canvas, 1);
            continue;
        }
        let action = Some(MenuAction::PlayFriend(index));
        let bounds = [left, y, right, y + row_height];
        row(canvas, view, bounds, false, action)?;
        let inner = canvas.r(1.6);
        let text_width = bounds[2] - bounds[0] - inner * 2.0;
        canvas.text(
            &friend.gamertag,
            [bounds[0] + inner, y + canvas.r(1.0)],
            text_width,
            BODY,
            TEXT,
            false,
        )?;
        let detail = format!("{} · {}", friend.world_name, friend.members);
        canvas.text(
            &detail,
            [bounds[0] + inner, y + canvas.r(3.4)],
            text_width,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
        y += row_height + space(canvas, 1);
    }
    let content = y + scroll.offset - list_top;
    canvas.end_scroll(scroll, content)
}
