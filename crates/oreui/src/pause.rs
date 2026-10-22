//! Pause keeps its action column separate from the player preview.

use super::{
    CharacterPreview,
    paint::Canvas,
    theme::{CAPTION, GUI_PIXELS_PER_REM, TEXT},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuScreen, MenuView};
use ui_engine::engine::screen::{PaintError, Translate};

#[cfg(test)]
mod tests;

const ACTION_HEIGHT: f32 = 28.0;
const ACTION_GAP: f32 = 3.0;
const PANEL_EXPANSION: f32 = 1.13;
const LEFT_PANEL_FRACTION: f32 = 0.55;
const MENU_FRACTION: f32 = 0.6;
const MENU_OFFSET: f32 = 0.13;

/// Draws the supported pause actions at a common GUI scale, fitting small windows as a group.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
) -> Result<Option<CharacterPreview>, PaintError> {
    let gui = canvas.rem / GUI_PIXELS_PER_REM;
    let compact = size[0] < gui * 360.0 || size[1] < gui * 230.0;
    let count = if compact { 4.0 } else { 3.0 };
    let width = if compact {
        (size[0] - gui * 8.0).min(gui * 148.0)
    } else {
        size[0] * PANEL_EXPANSION * LEFT_PANEL_FRACTION * MENU_FRACTION - gui * 10.0
    };
    let ratio = canvas.title_artwork.map_or(5.0, |icon| {
        f32::from(icon.uv[2] - icon.uv[0]) / f32::from((icon.uv[3] - icon.uv[1]).max(1))
    });
    let title = width / ratio;
    let required = title + gui * (4.0 + count * ACTION_HEIGHT + (count - 1.0) * ACTION_GAP + 16.0);
    let scale = ((size[1] - gui * 4.0).max(1.0) / required).min(1.0);
    canvas.with_scale(scale, |canvas| {
        draw_layout(canvas, view, size, translate, compact, width * scale, ratio)
    })
}

/// Paints a centered action stack inside the left-side shade and a separate wardrobe.
fn draw_layout(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
    compact: bool,
    width: f32,
    title_ratio: f32,
) -> Result<Option<CharacterPreview>, PaintError> {
    let gui = canvas.rem / GUI_PIXELS_PER_REM;
    let left = (1.0 - PANEL_EXPANSION) * size[0] * 0.5;
    let panel_width = size[0] * PANEL_EXPANSION * LEFT_PANEL_FRACTION;
    let center = if compact {
        size[0] * 0.5
    } else {
        left + panel_width * (0.5 + MENU_FRACTION * MENU_OFFSET)
    };
    canvas.overlay(size, [0, 0, 0, 26])?;
    canvas.fill(
        [
            0.0,
            0.0,
            if compact { size[0] } else { left + panel_width },
            size[1],
        ],
        [0, 0, 0, 128],
    )?;
    let count = if compact { 4.0 } else { 3.0 };
    let title_height = width / title_ratio;
    let height = title_height + gui * (4.0 + count * ACTION_HEIGHT + (count - 1.0) * ACTION_GAP);
    let x = center - width * 0.5;
    let right = center + width * 0.5;
    let mut y = (size[1] - height) * 0.5;
    let logo = [x, y, right, y + title_height];
    if let Some(icon) = canvas.title_artwork {
        canvas.icon_ref(icon, logo)?;
    } else {
        canvas.text_centred(
            launcher::PRODUCT_NAME,
            logo,
            super::theme::HEADER3,
            TEXT,
            false,
        )?;
    }
    y += title_height + gui * 4.0;
    for (label, action) in [
        ("Resume Game", MenuAction::PauseResume),
        ("Settings", MenuAction::PauseSettings),
    ] {
        widgets::button(
            canvas,
            view,
            [x, y, right, y + gui * ACTION_HEIGHT],
            Variant::Secondary,
            label,
            Some(action),
        )?;
        y += gui * (ACTION_HEIGHT + ACTION_GAP);
    }
    if compact {
        widgets::button(
            canvas,
            view,
            [x, y, right, y + gui * ACTION_HEIGHT],
            Variant::Secondary,
            "Dressing Room",
            Some(MenuAction::Navigate(MenuScreen::DressingRoom)),
        )?;
        y += gui * (ACTION_HEIGHT + ACTION_GAP);
    }
    let quit = translate("pauseScreen.quit").unwrap_or_else(|| "Save & Quit".into());
    widgets::button(
        canvas,
        view,
        [x, y, right, y + gui * ACTION_HEIGHT],
        Variant::Neutral,
        &quit,
        Some(MenuAction::PauseDisconnect),
    )?;
    if compact {
        return Ok(None);
    }
    let center = size[0] * 0.81;
    let width = gui * 80.0;
    let top = size[1] * 0.33;
    let bottom = size[1] * 0.72;
    let x = center - width * 0.5;
    let right = center + width * 0.5;
    canvas.text_centred(
        launcher::menu::profile::current_name(view),
        [x, top - gui * 12.0, right, top - gui * 2.0],
        CAPTION,
        TEXT,
        false,
    )?;
    widgets::button(
        canvas,
        view,
        [x, bottom, right, bottom + gui * 24.0],
        Variant::Secondary,
        "Dressing Room",
        Some(MenuAction::Navigate(MenuScreen::DressingRoom)),
    )?;
    let preview = [x, top, right, bottom - gui * 6.0];
    Ok(Some(CharacterPreview {
        control: preview,
        clip: preview,
    }))
}
