//! The play route (`/play/:tab`), OreUI by default in 26.30: the header, the
//! Worlds / Realms / Servers tab bar, and the Worlds tab (create buttons over
//! one flat list: friends' and LAN worlds first, then local worlds, each local
//! row playing on press with a trailing Edit action).

use std::collections::HashMap;
mod tabs;

use super::super::super::{IconRef, UiPresentationError};
use super::grid::{Grid, space};
use super::icons::{self, Icon};
use super::paint::{Bounds, Canvas};
use super::theme::EDGE;
use super::theme::{
    BODY, CAPTION, NEUTRAL80, NEUTRAL100, SECONDARY_BUTTON, TEXT, TEXT_DIMMER, TEXT_DIMMEST,
};
use super::widgets::{Variant, button, header, panel, row, screen_overlay, tag};
use super::{play_realms, play_servers};
use crate::menu::{LocalWorldAction, MenuAction, MenuScreen, MenuView};

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    images: &HashMap<String, IconRef>,
) -> Result<(), UiPresentationError> {
    let selected = match view.screen {
        MenuScreen::Social => 1,
        MenuScreen::Servers => 2,
        _ => 0,
    };
    draw_tab(canvas, view, size, images, selected)
}

pub(super) fn draw_tab(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    images: &HashMap<String, IconRef>,
    selected: usize,
) -> Result<(), UiPresentationError> {
    let [width, height] = size;
    screen_overlay(canvas, size)?;
    let top = header(
        canvas,
        view,
        "Play",
        width,
        Some(MenuAction::Navigate(MenuScreen::Home)),
    )?;
    let grid = Grid::new(canvas.r(1.0), width);
    let [left, right] = grid.span(0, if grid.narrow { 8 } else { 12 });
    let tab_top = top + space(canvas, 1);
    let tab_bottom = tab_top + canvas.r(4.8);
    tabs::draw(canvas, view, [left, tab_top, right, tab_bottom], selected)?;
    let body: Bounds = [
        left,
        tab_bottom + space(canvas, 2),
        right,
        height - space(canvas, 2),
    ];
    let entrance = canvas.begin_entrance(super::motion::Surface::Play(selected as u8));
    match selected {
        1 => play_realms::draw(canvas, view, &grid, body),
        2 => play_servers::draw(canvas, view, &grid, body, images),
        _ => worlds_tab(canvas, view, body),
    }?;
    canvas.end_entrance(entrance, size)
}

fn worlds_tab(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    body: Bounds,
) -> Result<(), UiPresentationError> {
    // Create buttons, right-aligned, each at most 25.6rem.
    let button_height = canvas.r(4.4);
    let gap = space(canvas, 2);
    let button_width = canvas.r(25.6).min((body[2] - body[0] - gap) / 2.0);
    let second = body[2] - button_width;
    let first = second - gap - button_width;
    button(
        canvas,
        view,
        [
            first,
            body[1],
            first + button_width,
            body[1] + button_height,
        ],
        Variant::Primary,
        "Create new world",
        Some(MenuAction::LocalWorld(LocalWorldAction::BeginCreate)),
    )?;
    button(
        canvas,
        view,
        [second, body[1], body[2], body[1] + button_height],
        Variant::Secondary,
        "Create from template",
        Some(MenuAction::LocalWorld(LocalWorldAction::OpenTemplates)),
    )?;
    let list_top = body[1] + button_height + space(canvas, 1);
    if view.friends.is_empty() && view.local_worlds.is_empty() {
        let card = [body[0], list_top, body[2], list_top + canvas.r(18.0)];
        panel(canvas, card)?;
        let pad = space(canvas, 4);
        canvas.text_centred(
            "No worlds here... yet!",
            [
                card[0],
                card[1] + pad,
                card[2],
                card[1] + pad + canvas.r(2.4),
            ],
            SECONDARY_BUTTON,
            TEXT,
            false,
        )?;
        canvas.text_centred(
            "Create a new world from scratch or create a world from a template.",
            [
                card[0],
                card[3] - pad - canvas.r(2.4),
                card[2],
                card[3] - pad,
            ],
            CAPTION,
            TEXT_DIMMEST,
            false,
        )?;
        return Ok(());
    }
    let row_height = canvas.r(8.4);
    let gap = space(canvas, 2);
    let scroll = canvas.begin_scroll("worlds_list", [body[0], list_top, body[2], body[3]])?;
    let mut y = list_top - scroll.offset;
    let entries = view
        .friends
        .iter()
        .enumerate()
        .map(|(index, friend)| WorldEntry {
            title: friend.world_name.clone(),
            subtitle: format!("{}'s world", friend.gamertag),
            tag: "Friend's world",
            world_type: String::new(),
            meta: [friend.members.clone(), String::new()],
            action: MenuAction::PlayFriend(index),
            edit: None,
        })
        .chain(
            view.local_worlds
                .iter()
                .enumerate()
                .map(|(index, world)| WorldEntry {
                    title: world.name.clone(),
                    subtitle: String::new(),
                    tag: game_mode_tag(&world.game_mode),
                    world_type: world.world_type.clone(),
                    meta: [world.size.clone(), world.date.clone()],
                    action: MenuAction::PlayLocalWorld(index),
                    edit: Some(MenuAction::LocalWorld(LocalWorldAction::Edit(index))),
                }),
        );
    for entry in entries {
        if y + row_height < list_top || y > body[3] {
            y += row_height + gap;
            continue;
        }
        world_row(canvas, view, &entry, [body[0], y, body[2], y + row_height])?;
        y += row_height + gap;
    }
    let content = y + scroll.offset - list_top;
    canvas.end_scroll(scroll, content)
}

struct WorldEntry {
    title: String,
    subtitle: String,
    tag: &'static str,
    /// A local world's type tag, beside its game mode.
    world_type: String,
    meta: [String; 2],
    action: MenuAction,
    /// The trailing pencil-and-"Edit" action of an owned world.
    edit: Option<MenuAction>,
}

fn game_mode_tag(mode: &str) -> &'static str {
    match mode.to_ascii_lowercase().as_str() {
        "creative" => "Creative",
        "adventure" => "Adventure",
        _ => "Survival",
    }
}

/// One list row: 16:9 thumbnail, title, subtitle and tag, metadata on the right.
fn world_row(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    entry: &WorldEntry,
    b: Bounds,
) -> Result<(), UiPresentationError> {
    row(canvas, view, b, false, Some(entry.action))?;
    let pad = canvas.r(0.8);
    let thumb_width = canvas.r(12.0);
    let thumb = [
        b[0] + pad,
        b[1] + pad,
        b[0] + pad + thumb_width,
        b[1] + pad + thumb_width * 9.0 / 16.0,
    ];
    canvas.fill(thumb, NEUTRAL100)?;
    canvas.frame(thumb, 0.2, [0x1e, 0x1e, 0x1f, 255])?;
    let text_left = thumb[2] + canvas.r(0.8);
    let edit_width = if entry.edit.is_some() {
        canvas.r(10.0)
    } else {
        0.0
    };
    let meta_width = canvas.r(12.0) + edit_width;
    let text_width = (b[2] - pad - meta_width - text_left).max(canvas.r(4.0));
    let mut y = b[1] + pad;
    y += canvas.text(&entry.title, [text_left, y], text_width, BODY, TEXT, false)?;
    if !entry.subtitle.is_empty() {
        y += canvas.text(
            &entry.subtitle,
            [text_left, y],
            text_width,
            BODY,
            TEXT_DIMMER,
            false,
        )?;
    }
    let tag_right = tag(
        canvas,
        entry.tag,
        [text_left, y + canvas.r(0.4)],
        NEUTRAL80.fill,
        TEXT,
    )?;
    if !entry.world_type.is_empty() {
        tag(
            canvas,
            &entry.world_type,
            [tag_right + canvas.r(0.4), y + canvas.r(0.4)],
            NEUTRAL80.fill,
            TEXT,
        )?;
    }
    let meta_right = b[2] - pad - edit_width;
    let mut meta_top = b[1] + pad;
    for line in entry.meta.iter().filter(|line| !line.is_empty()) {
        let width = canvas.measure(line, CAPTION)?;
        meta_top += canvas.text(
            line,
            [meta_right - canvas.r(1.0) - width, meta_top],
            width + 1.0,
            CAPTION,
            TEXT_DIMMEST,
            false,
        )?;
    }
    if let Some(edit) = entry.edit {
        let edit_bounds = [b[2] - pad - edit_width, b[1] + pad, b[2] - pad, b[3] - pad];
        edit_action(canvas, view, edit_bounds, edit)?;
    }
    Ok(())
}

/// The row's trailing Edit action: a pencil over its label, lit on hover.
fn edit_action(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    action: MenuAction,
) -> Result<(), UiPresentationError> {
    row(canvas, view, b, false, Some(action))?;
    let [w, h] = Icon::Pencil.texels();
    let texel = canvas.r(EDGE);
    let label_height = canvas.r(BODY.line);
    let block = h as f32 * texel + canvas.r(0.4) + label_height;
    let top = (b[1] + b[3] - block) * 0.5;
    icons::draw(
        canvas,
        Icon::Pencil,
        [(b[0] + b[2] - w as f32 * texel) * 0.5, top],
        TEXT,
    )?;
    let label_top = top + h as f32 * texel + canvas.r(0.4);
    canvas.text_centred(
        "Edit",
        [b[0], label_top, b[2], label_top + label_height],
        BODY,
        TEXT,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_tags_name_the_game_mode() {
        assert_eq!(game_mode_tag("Creative"), "Creative");
        assert_eq!(game_mode_tag("adventure"), "Adventure");
        assert_eq!(game_mode_tag("anything"), "Survival");
    }
}
