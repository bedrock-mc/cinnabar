//! The local-world routes 1.26.50 draws with OreUI: Create New World (`/create-new-world`),
//! Edit world (`/edit-world`) and Create From Template (`/start-from-template`). Each is the
//! header with back, a side menu in four of twelve columns (preview, hero button, tab list)
//! and the tab's controls in eight. Unsupported settings remain visibly disabled.

use protocol::world_control::{Difficulty, GameMode, Generator};

use super::super::super::UiPresentationError;
use super::grid::{Grid, space};
use super::paint::{Bounds, Canvas};
use super::theme::{
    BODY, BORDER, CAPTION, DESTRUCTIVE, EDGE, SECONDARY_BUTTON, TEXT, TEXT_DIMMER, TEXT_DIMMEST,
};
use super::widgets::{
    Variant, button, header, panel, row, screen_overlay, segmented, text_field_on_panel,
};
use crate::local_worlds::{
    Screen, Tab, WorldsView, difficulty_description, difficulty_label, game_mode_description,
    game_mode_label, world_type_label,
};
use crate::menu::{LocalWorldAction as A, MenuAction, MenuField, MenuView};

mod advanced;
mod general;
mod sections;
mod sidebar;
mod templates;

const FIELD: f32 = 4.8;
const CONTROL: f32 = 4.4;

#[cfg(test)]
mod tests;

/// Wraps a world-form action for shared menu navigation.
fn local(action: A) -> MenuAction {
    MenuAction::LocalWorld(action)
}

/// Which route a local-world state draws under its modal, if it covers the worlds tab.
pub(super) fn route(screen: Screen, view: &WorldsView) -> Option<Screen> {
    match screen {
        Screen::Create | Screen::Edit | Screen::Templates => Some(screen),
        Screen::ConfirmDelete | Screen::ConfirmLeaveEdit => Some(Screen::Edit),
        Screen::BackendPrompt
            if view
                .prompt
                .is_some_and(|p| p.blocking == crate::local_worlds::PromptFor::CreateBds) =>
        {
            Some(Screen::Create)
        }
        _ => None,
    }
}

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    route: Screen,
) -> Result<(), UiPresentationError> {
    let [width, height] = size;
    screen_overlay(canvas, size)?;
    let title = match route {
        Screen::Edit => "Edit world",
        Screen::Templates => "Create From Template",
        _ => "Create New World",
    };
    let mut top = header(canvas, view, title, width, Some(local(A::Back)))? + space(canvas, 2);
    let grid = Grid::new(canvas.r(1.0), width);
    let (side, content) = if grid.narrow {
        ((0, 3), (3, 5))
    } else {
        ((0, 4), (4, 8))
    };
    let side = grid.span(side.0, side.1);
    let content = grid.span(content.0, content.1);
    let bottom = height - space(canvas, 2);
    if route == Screen::Templates {
        let span = grid.span(0, if grid.narrow { 8 } else { 12 });
        let scroll = canvas.begin_scroll("world_templates", [span[0], top, span[1], bottom])?;
        top -= scroll.offset;
        button(
            canvas,
            view,
            [side[0], top, side[1], top + canvas.r(CONTROL)],
            Variant::Primary,
            "Create new world",
            Some(local(A::BeginCreate)),
        )?;
        tab_row(
            canvas,
            view,
            side,
            top + canvas.r(CONTROL) + space(canvas, 2),
            "Owned by me (0)",
            true,
            None,
        )?;
        templates::draw(canvas, view, [content[0], top, content[1], bottom])?;
        return canvas.end_scroll_to_fit(scroll);
    }
    canvas.settings_scrollbars = true;
    sidebar::draw(
        canvas,
        view,
        [side[0], top, side[1], bottom],
        route,
        grid.narrow,
    )?;
    let key = match (route, view.local.tab) {
        (Screen::Edit, _) => "world_edit_general",
        (_, Tab::General) => "world_create_general",
        (_, Tab::Advanced) => "world_create_advanced",
    };
    let scroll = canvas.begin_scroll(key, [content[0], top, content[1], bottom])?;
    let entrance = canvas.begin_entrance(super::motion::Surface::WorldTab(route, view.local.tab));
    let area = [content[0], top - scroll.offset, content[1], bottom];
    let end = if route == Screen::Edit {
        general::edit(canvas, view, &view.local, area)?
    } else {
        match view.local.tab {
            Tab::General => general::create(canvas, view, &view.local, area)?,
            Tab::Advanced => advanced::draw(canvas, view, &view.local, area)?,
        }
    };
    canvas.frame([content[0], area[1], content[1], end], EDGE, BORDER)?;
    canvas.end_entrance(entrance, size)?;
    canvas.end_scroll(scroll, end - area[1])
}

/// One side-menu tab row; returns the y below it.
fn tab_row(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    side: [f32; 2],
    top: f32,
    label: &str,
    selected: bool,
    action: Option<MenuAction>,
) -> Result<f32, UiPresentationError> {
    let height = canvas.r(4.8);
    let b = [side[0], top, side[1], top + height];
    row(canvas, view, b, selected, action)?;
    canvas.text_line(
        label,
        [
            b[0] + canvas.r(1.6),
            top + (height - canvas.r(BODY.line)) * 0.5,
        ],
        b[2] - b[0] - canvas.r(3.2),
        BODY,
        TEXT,
    )?;
    Ok(b[3] + space(canvas, 1))
}

/// A control's label above it; returns the y below.
fn label(
    canvas: &mut Canvas<'_>,
    text: &str,
    area: Bounds,
    y: f32,
) -> Result<f32, UiPresentationError> {
    let height = canvas.text(text, [area[0], y], area[2] - area[0], BODY, TEXT, false)?;
    Ok(y + height + space(canvas, 1))
}

/// A description under a control; returns its bottom edge.
fn caption(
    canvas: &mut Canvas<'_>,
    text: &str,
    area: Bounds,
    y: f32,
) -> Result<f32, UiPresentationError> {
    let height = canvas.text(
        text,
        [area[0], y],
        area[2] - area[0],
        CAPTION,
        TEXT_DIMMEST,
        false,
    )?;
    Ok(y + height)
}

/// Draws the name field and its validation feedback.
fn name_field(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    name: &str,
    area: Bounds,
    y: f32,
) -> Result<f32, UiPresentationError> {
    let y = label(canvas, "World name", area, y)?;
    let focused = view.field == Some(MenuField::WorldName);
    let b = [area[0], y, area[2], y + canvas.r(FIELD)];
    text_field_on_panel(
        canvas,
        view,
        b,
        name,
        "My World",
        focused,
        Some(local(A::NameField)),
    )?;
    let mut y = b[3] + space(canvas, 1);
    if let Some(error) = view.local.form_error {
        y += canvas.text(
            error,
            [area[0], y],
            area[2] - area[0],
            CAPTION,
            DESTRUCTIVE.fill,
            false,
        )?;
    }
    Ok(y)
}

/// Draws the available game modes and their selected description.
fn game_modes(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    modes: &[GameMode],
    current: GameMode,
    area: Bounds,
    y: f32,
) -> Result<f32, UiPresentationError> {
    let y = label(canvas, "Game mode", area, y)?;
    let options: Vec<_> = modes
        .iter()
        .map(|mode| {
            (
                game_mode_label(*mode),
                local(A::GameMode(*mode)),
                *mode == current,
            )
        })
        .collect();
    let height = choice_height(canvas, area, &options)?;
    segmented(canvas, view, [area[0], y, area[2], y + height], &options)?;
    caption(
        canvas,
        game_mode_description(current),
        area,
        y + height + space(canvas, 2),
    )
}

/// Draws the difficulty choices and selected description.
fn difficulties(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    current: Difficulty,
    area: Bounds,
    y: f32,
) -> Result<f32, UiPresentationError> {
    let y = label(canvas, "Difficulty", area, y)?;
    let options: Vec<_> = [
        Difficulty::Peaceful,
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Hard,
    ]
    .iter()
    .map(|d| {
        (
            difficulty_label(*d),
            local(A::Difficulty(*d)),
            *d == current,
        )
    })
    .collect();
    let height = choice_height(canvas, area, &options)?;
    segmented(canvas, view, [area[0], y, area[2], y + height], &options)?;
    caption(
        canvas,
        difficulty_description(current),
        area,
        y + height + space(canvas, 2),
    )
}

/// Keeps each segmented choice tall enough for wrapped labels.
fn choice_height(
    canvas: &mut Canvas<'_>,
    area: Bounds,
    options: &[(&str, MenuAction, bool)],
) -> Result<f32, UiPresentationError> {
    let width = (area[2] - area[0]) / options.len().max(1) as f32;
    let mut height = canvas.r(5.6);
    for (label, _, _) in options {
        height = height.max(super::widgets::choice_height(canvas, label, width)?);
    }
    Ok(height)
}
