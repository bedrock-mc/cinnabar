//! An equipped character beside its skin and cape wardrobe.

mod character;
mod editor;
mod filter;
mod hats;
mod library;
#[cfg(test)]
mod tests;

use super::grid::{Grid, space};
use super::paint::{Bounds, Canvas};
use super::widgets;
use launcher::dressing_room::Action;
use launcher::menu::{MenuAction, MenuView};
use ui_engine::engine::screen::PaintError;

pub(super) struct PreviewArea {
    pub(super) control: Bounds,
    pub(super) clip: Bounds,
    pub(super) visible_skins: Vec<usize>,
    pub(super) visible_capes: Vec<usize>,
    pub(super) visible_cosmetics: Vec<usize>,
    pub(super) filter_anchor: Option<Bounds>,
}

fn command(action: Action) -> MenuAction {
    MenuAction::DressingRoom(action)
}
fn enabled(view: &MenuView) -> bool {
    !view.dressing_room.busy && view.dressing_room.editor.is_none()
}

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<PreviewArea, PaintError> {
    widgets::screen_overlay(canvas, size)?;
    let top = widgets::header(
        canvas,
        view,
        "Dressing Room",
        size[0],
        view.dressing_room
            .editor
            .is_none()
            .then_some(MenuAction::AddBack),
    )? + space(canvas, 4);
    let bottom = size[1] - space(canvas, 4);
    let grid = Grid::new(canvas.rem, size[0]);
    if grid.narrow || bottom - top < canvas.r(44.0) {
        let [left, right] = grid.span(0, if grid.narrow { 8 } else { 12 });
        let viewport = [left, top, right, bottom.max(top + canvas.r(4.0))];
        let scroll = canvas.begin_scroll(&scroll_key(view, true), viewport)?;
        let start = top - scroll.offset;
        let height = if view
            .dressing_room
            .selected_skin()
            .is_some_and(|skin| skin.imported)
        {
            58.0
        } else {
            49.0
        };
        let preview_bottom = start + canvas.r(height);
        let mut preview =
            character::draw(canvas, view, [left, start, right, preview_bottom], viewport)?;
        let end = library::draw(
            canvas,
            view,
            [left, preview_bottom + space(canvas, 4), right, bottom],
            viewport,
            &mut preview,
        )?;
        canvas.end_scroll(scroll, end - start + space(canvas, 2))?;
        return Ok(preview);
    }
    let [left, right] = grid.span(0, 5);
    let [library_left, library_right] = grid.span(5, 7);
    let mut preview = character::draw(
        canvas,
        view,
        [left, top, right, bottom],
        [left, top, right, bottom],
    )?;
    widgets::panel(canvas, [library_left, top, library_right, bottom])?;
    let pad = space(canvas, 4);
    let body_top = library::toolbar(
        canvas,
        view,
        [
            library_left + pad,
            top + pad,
            library_right - pad - space(canvas, 2),
            bottom,
        ],
    )?;
    let viewport = [
        library_left + pad,
        body_top,
        library_right - pad,
        bottom - pad,
    ];
    let scroll = canvas.begin_scroll(&scroll_key(view, false), viewport)?;
    let start = viewport[1] - scroll.offset;
    let end = library::collection(
        canvas,
        view,
        [viewport[0], start, viewport[2] - space(canvas, 2), bottom],
        viewport,
        &mut preview,
    )?;
    canvas.end_scroll(scroll, end - start + space(canvas, 2))?;
    Ok(preview)
}

/// Retains separate scroll positions for sections, layouts and cosmetic categories.
fn scroll_key(view: &MenuView, stacked: bool) -> std::borrow::Cow<'static, str> {
    use launcher::dressing_room::{CosmeticCategory, DressingRoomSection};
    let base = match (view.dressing_room.section, stacked) {
        (DressingRoomSection::Skins, true) => "dressing_room",
        (DressingRoomSection::Capes, true) => "dressing_room_capes",
        (DressingRoomSection::Cosmetics, true) => "dressing_room_hats",
        (DressingRoomSection::Skins, false) => "skin_library",
        (DressingRoomSection::Capes, false) => "cape_library",
        (DressingRoomSection::Cosmetics, false) => "hat_library",
    };
    let category = view.dressing_room.cosmetic_category;
    if view.dressing_room.section == DressingRoomSection::Cosmetics
        && category != CosmeticCategory::All
    {
        return format!("{base}:{}", category.label()).into();
    }
    base.into()
}

/// Paints the active wardrobe overlay after the character preview.
pub(super) fn draw_overlay(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    filter_anchor: Option<Bounds>,
) -> Result<(), PaintError> {
    if view.dressing_room.editor.is_some() {
        editor::draw(canvas, view, size)
    } else if view.dressing_room.cosmetic_filter_open
        && enabled(view)
        && let Some(anchor) = filter_anchor
    {
        filter::popup(canvas, view, size, anchor)
    } else {
        Ok(())
    }
}
