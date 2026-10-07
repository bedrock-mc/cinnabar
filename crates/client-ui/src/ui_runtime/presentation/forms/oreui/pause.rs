use super::super::super::UiPresentationError;
use super::{
    CharacterPreview,
    paint::Canvas,
    theme::{CAPTION, HEADER5, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use crate::menu::{MenuAction, MenuScreen, MenuView};

/// A uniform world overlay frames the actions and the interactive character together.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<Option<CharacterPreview>, UiPresentationError> {
    widgets::screen_overlay(canvas, size)?;
    let margin = canvas.r(2.4).min(size[0] * 0.04).min(size[1] * 0.025);
    let width = canvas.r(78.0).min(size[0] - margin * 2.0);
    let compact = width < canvas.r(58.0) || size[1] < canvas.r(44.0);
    let pad = canvas.r(if compact { 1.6 } else { 2.4 });
    let vertical_pad = if compact { pad } else { canvas.r(3.2) };
    let gap = canvas.r(1.2);
    let action_height = canvas.r(4.8);
    let height = canvas
        .r(if compact { 32.0 } else { 49.6 })
        .min(size[1] - margin * 2.0);
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
        y += canvas.r(0.8);
        let box_height = canvas.r(8.8);
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
        y += box_height + canvas.r(2.0);
    }
    let count = if compact { 4.0 } else { 3.0 };
    let heading_height = canvas.r(HEADER5.line + 0.8);
    let caption_height = canvas.r(CAPTION.line + 1.6);
    if !compact {
        let group_height =
            heading_height + caption_height + action_height * count + gap * (count - 1.0);
        y = y.max(bottom - vertical_pad - group_height);
    }
    canvas.text_line("GAME MENU", [x, y], right - x, HEADER5, TEXT)?;
    y += heading_height;
    canvas.text_line(
        "Pick up where you left off.",
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
    widgets::button(
        canvas,
        view,
        [x, y, right, y + button_height],
        Variant::Neutral,
        "Leave world",
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
        super::super::accounts::current_name(view),
        [cx, top + vertical_pad + canvas.r(HEADER5.line + 0.8)],
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
