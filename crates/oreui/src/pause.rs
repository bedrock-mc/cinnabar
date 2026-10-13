use super::{
    CharacterPreview,
    paint::Canvas,
    theme::{CAPTION, HEADER5, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuScreen, MenuView};
use ui_engine::engine::screen::PaintError;
use ui_engine::engine::screen::Translate;

#[cfg(test)]
mod tests;

const ACTION_HEIGHT: f32 = 4.8;
const ACTION_GAP: f32 = 1.2;
const COMPACT_PADDING: f32 = 1.6;
const VERTICAL_PADDING: f32 = 3.2;
const LOGO_TOP_GAP: f32 = 0.8;
const LOGO_HEIGHT: f32 = 8.8;
const LOGO_BOTTOM_GAP: f32 = 2.0;
const HEADING_HEIGHT: f32 = HEADER5.line + 0.8;
const CAPTION_HEIGHT: f32 = CAPTION.line + 1.6;

/// The compact card includes the wardrobe among its actions.
fn action_count(compact: bool) -> f32 {
    if compact { 4.0 } else { 3.0 }
}

/// The space needed for the heading, full-height actions and optional title art, in rem.
fn minimum_height(compact: bool, title: bool) -> f32 {
    let count = action_count(compact);
    let padding = if compact {
        COMPACT_PADDING
    } else {
        VERTICAL_PADDING
    };
    let logo = if !compact && title {
        LOGO_TOP_GAP + LOGO_HEIGHT + LOGO_BOTTOM_GAP
    } else {
        0.0
    };
    padding * 2.0
        + logo
        + HEADING_HEIGHT
        + CAPTION_HEIGHT
        + ACTION_HEIGHT * count
        + ACTION_GAP * (count - 1.0)
}

/// A uniform world overlay frames the actions and the interactive character together.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
) -> Result<Option<CharacterPreview>, PaintError> {
    let margin = canvas.r(2.4).min(size[0] * 0.04).min(size[1] * 0.025);
    let width = canvas.r(78.0).min(size[0] - margin * 2.0);
    let compact = width < canvas.r(58.0) || size[1] < canvas.r(44.0);
    let minimum = minimum_height(compact, canvas.title_artwork.is_some());
    let scale = ((size[1] - margin * 2.0) / canvas.r(minimum)).min(1.0);
    canvas.with_scale(scale, |canvas| {
        draw_layout(canvas, view, size, translate, margin, compact, minimum)
    })
}

/// Draws the pause group with its original layout and a shared geometry and text scale.
fn draw_layout(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
    margin: f32,
    compact: bool,
    minimum: f32,
) -> Result<Option<CharacterPreview>, PaintError> {
    widgets::screen_overlay(canvas, size)?;
    let width = canvas.r(78.0).min(size[0] - margin * 2.0);
    let pad = canvas.r(if compact { COMPACT_PADDING } else { 2.4 });
    let vertical_pad = if compact {
        pad
    } else {
        canvas.r(VERTICAL_PADDING)
    };
    let gap = canvas.r(ACTION_GAP);
    let action_height = canvas.r(ACTION_HEIGHT);
    let height = canvas
        .r(if compact { 32.0 } else { 49.6 })
        .min(size[1] - margin * 2.0)
        .max(canvas.r(minimum));
    let left = (size[0] - width) * 0.5;
    let top = (size[1] - height) * 0.5;
    let bottom = top + height;
    let action_right = if compact {
        left + width
    } else {
        left + width * 0.54
    };
    widgets::panel(canvas, [left, top, action_right, bottom])?;
    let x = left + pad;
    let right = action_right - pad;
    let mut y = top + vertical_pad;
    if !compact && let Some(icon) = canvas.title_artwork {
        y += canvas.r(LOGO_TOP_GAP);
        let box_height = canvas.r(LOGO_HEIGHT);
        let ratio =
            f32::from(icon.uv[2] - icon.uv[0]) / f32::from((icon.uv[3] - icon.uv[1]).max(1));
        let art_width = (box_height * ratio).min(right - x);
        let art_height = art_width / ratio;
        let art_left = (x + right - art_width) * 0.5;
        canvas.icon_ref(
            icon,
            [
                art_left,
                y + (box_height - art_height) * 0.5,
                art_left + art_width,
                y + (box_height + art_height) * 0.5,
            ],
        )?;
        y += box_height + canvas.r(LOGO_BOTTOM_GAP);
    }
    let count = action_count(compact);
    let heading_height = canvas.r(HEADING_HEIGHT);
    let caption_height = canvas.r(CAPTION_HEIGHT);
    if !compact {
        let group_height =
            heading_height + caption_height + action_height * count + gap * (count - 1.0);
        y = y.max(bottom - vertical_pad - group_height);
    }
    canvas.text_line("GAME MENU", [x, y], right - x, HEADER5, TEXT)?;
    y += heading_height;
    canvas.text_line(
        if view.updates.ready.is_some() {
            "Update ready"
        } else {
            "Pick up where you left off."
        },
        [x, y],
        right - x,
        CAPTION,
        TEXT_DIMMER,
    )?;
    let button_height = action_height
        .min((bottom - vertical_pad - y - caption_height - gap * (count - 1.0)) / count);
    y += caption_height;
    for (label, variant, action) in [
        ("Resume game", Variant::Hero, MenuAction::PauseResume),
        ("Settings", Variant::Secondary, MenuAction::PauseSettings),
    ] {
        widgets::button(
            canvas,
            view,
            [x, y, right, y + button_height],
            variant,
            label,
            Some(action),
        )?;
        y += button_height + gap;
    }
    if compact {
        widgets::button(
            canvas,
            view,
            [x, y, right, y + button_height],
            Variant::Neutral,
            "Dressing Room",
            Some(MenuAction::Navigate(MenuScreen::DressingRoom)),
        )?;
        y += button_height + gap;
    }
    let quit = translate("pauseScreen.quit").unwrap_or_else(|| "Save & Quit".into());
    widgets::button(
        canvas,
        view,
        [x, y, right, y + button_height],
        Variant::Neutral,
        &quit,
        Some(MenuAction::PauseDisconnect),
    )?;
    if compact {
        return Ok(None);
    }
    let character = [action_right + canvas.r(1.6), top, left + width, bottom];
    widgets::panel(canvas, character)?;
    let cx = character[0] + pad;
    let cr = character[2] - pad;
    canvas.text_line(
        "YOUR CHARACTER",
        [cx, top + vertical_pad],
        cr - cx,
        HEADER5,
        TEXT,
    )?;
    canvas.text_line(
        launcher::menu::profile::current_name(view),
        [cx, top + vertical_pad + heading_height],
        cr - cx,
        CAPTION,
        TEXT_DIMMER,
    )?;
    let footer_top = bottom - vertical_pad - action_height;
    widgets::button(
        canvas,
        view,
        [cx, footer_top, cr, bottom - vertical_pad],
        Variant::Secondary,
        "Dressing Room",
        Some(MenuAction::Navigate(MenuScreen::DressingRoom)),
    )?;
    let hint = [
        cx,
        footer_top - canvas.r(3.6),
        cr,
        footer_top - canvas.r(1.2),
    ];
    canvas.text_centred("Drag to rotate", hint, CAPTION, TEXT_DIMMER, false)?;
    let preview = [
        cx,
        top + vertical_pad + canvas.r(7.2),
        cr,
        hint[1] - canvas.r(0.8),
    ];
    Ok(Some(CharacterPreview {
        control: preview,
        clip: preview,
    }))
}
