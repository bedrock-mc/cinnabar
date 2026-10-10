//! The owner's home layout keeps play, identity and utility actions together.

mod footer;
#[cfg(test)]
mod tests;

use super::super::menu_screens::Translate;
use super::{
    CharacterPreview, focus,
    motion::{Kind, mix, opacity},
    paint::{Bounds, Canvas},
    theme::{CAPTION, EDGE, HEADER5, NEUTRAL80, OUTLINE, TEXT, TEXT_DIMMER, TEXT_DIMMEST},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuScreen, MenuView};
use {super::super::super::UiPresentationError, ui::IconRef};

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    portrait: Option<IconRef>,
    translate: Translate<'_>,
) -> Result<Option<CharacterPreview>, UiPresentationError> {
    canvas.overlay(size, [0, 0, 0, 96])?;
    let margin = canvas.r(3.2).min(size[0] * 0.04).min(size[1] * 0.035);
    let width = canvas.r(86.0).min(size[0] - margin * 2.0);
    let compact = width < canvas.r(68.0) || size[1] < canvas.r(44.0);
    let compact_footer = width < canvas.r(60.0);
    let gap = canvas.r(1.6);
    let footer_height = canvas.r(if compact_footer { 10.8 } else { 4.8 });
    let version_height = canvas.r(if compact_footer { 4.0 } else { 2.8 });
    let logo_width = canvas.r(56.0).min(width * 0.78);
    let logo_height = canvas.title_artwork.map_or(canvas.r(6.4), |icon| {
        let ratio =
            f32::from(icon.uv[2] - icon.uv[0]) / f32::from((icon.uv[3] - icon.uv[1]).max(1));
        logo_width / ratio
    });
    let outside = logo_height + gap * 2.0 + footer_height + version_height;
    let height = canvas.r(36.8).min(size[1] - margin * 2.0 - outside);
    let left = (size[0] - width) * 0.5;
    let group_top = (size[1] - height - outside) * 0.5;
    let logo_left = (size[0] - logo_width) * 0.5;
    let logo_bounds = [
        logo_left,
        group_top,
        logo_left + logo_width,
        group_top + logo_height,
    ];
    if let Some(icon) = canvas.title_artwork {
        canvas.icon_ref(icon, logo_bounds)?;
    } else {
        canvas.text_centred(
            launcher::PRODUCT_NAME,
            logo_bounds,
            super::theme::HEADER3,
            TEXT,
            false,
        )?;
    }
    let top = group_top + logo_height + gap;
    let bottom = top + height;
    let action_right = if compact {
        left + width
    } else {
        left + width * 0.59
    };
    let screen = canvas.begin_focus_region(
        focus::SCREEN,
        [left, group_top, left + width, size[1] - group_top],
        None,
        true,
    )?;
    canvas.focus_delegate(Some(MenuAction::Navigate(MenuScreen::Play)), None);
    let actions = [left, top, action_right, bottom];
    action_panel(canvas, view, actions, compact, translate)?;
    let preview = if compact {
        None
    } else {
        Some(character_panel(
            canvas,
            view,
            [action_right + gap, top, left + width, bottom],
        )?)
    };
    footer::draw(
        canvas,
        view,
        [
            left,
            bottom + gap,
            left + width,
            bottom + gap + footer_height,
        ],
        compact_footer,
        portrait,
    )?;
    let version_bounds = [
        left,
        bottom + gap + footer_height + canvas.r(0.6),
        left + width,
        size[1] - group_top,
    ];
    let style = super::theme::Type {
        size: 1.1,
        line: 1.6,
        ..CAPTION
    };
    let app = format!("{} {}", launcher::PRODUCT_NAME, launcher::PRODUCT_VERSION);
    let game = format!(
        "Minecraft {} / Protocol {}",
        protocol::GAME_VERSION,
        protocol::PROTOCOL_VERSION
    );
    if compact_footer {
        let middle = (version_bounds[1] + version_bounds[3]) * 0.5;
        canvas.text_centred(
            &app,
            [
                version_bounds[0],
                version_bounds[1],
                version_bounds[2],
                middle,
            ],
            style,
            TEXT_DIMMEST,
            false,
        )?;
        canvas.text_centred(
            &game,
            [
                version_bounds[0],
                middle,
                version_bounds[2],
                version_bounds[3],
            ],
            style,
            TEXT_DIMMEST,
            false,
        )?;
    } else {
        canvas.text_centred(
            &format!("{app}  /  {game}"),
            version_bounds,
            style,
            TEXT_DIMMEST,
            false,
        )?;
    }
    canvas.end_focus_region(screen);
    Ok(preview)
}

fn action_panel(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    compact: bool,
    translate: Translate<'_>,
) -> Result<(), UiPresentationError> {
    widgets::panel(canvas, bounds)?;
    let pad = canvas
        .r(if compact { 2.0 } else { 3.2 })
        .min((bounds[2] - bounds[0]) * 0.08);
    let viewport = [
        bounds[0] + canvas.r(EDGE),
        bounds[1] + canvas.r(EDGE),
        bounds[2] - canvas.r(EDGE),
        bounds[3] - canvas.r(EDGE),
    ];
    let content = canvas.begin_focus_region(
        focus::CONTENT,
        viewport,
        Some(launcher::menu::view::SettingsFocusAxis::Vertical),
        true,
    )?;
    canvas.focus_delegate(Some(MenuAction::Navigate(MenuScreen::Play)), None);
    let scroll = canvas.begin_scroll("oreui_home/actions", viewport)?;
    let event_height = if view.feeds.home.live_event.is_some() {
        5.2
    } else {
        0.0
    };
    let group_height = canvas.r(if compact { 30.0 } else { 24.0 });
    let content_height =
        (bounds[3] - bounds[1]).max(group_height + pad * 2.0 + canvas.r(event_height));
    let origin = bounds[1] - scroll.offset;
    let x = bounds[0] + pad;
    let right = bounds[2] - pad;
    let mut y = origin
        + pad
        + canvas.r(event_height)
        + (content_height - pad * 2.0 - group_height - canvas.r(event_height)).max(0.0) * 0.5;
    if let Some(event) = &view.feeds.home.live_event {
        let b = [x, y - canvas.r(5.2), right, y - canvas.r(1.2)];
        live_event(canvas, view, b, event, translate)?;
    }
    canvas.text_line("LET'S PLAY", [x, y], right - x, HEADER5, TEXT)?;
    y += canvas.r(HEADER5.line + 0.8);
    canvas.text_line(
        "Your worlds, friends and favorite servers.",
        [x, y],
        right - x,
        CAPTION,
        TEXT_DIMMER,
    )?;
    y += canvas.r(CAPTION.line + 1.6);
    widgets::button(
        canvas,
        view,
        [x, y, right, y + canvas.r(6.4)],
        Variant::Hero,
        "Play",
        Some(MenuAction::Navigate(MenuScreen::Play)),
    )?;
    y += canvas.r(7.6);
    let midpoint = (x + right) * 0.5;
    let gap = canvas.r(1.2);
    widgets::button(
        canvas,
        view,
        [x, y, midpoint - gap * 0.5, y + canvas.r(4.8)],
        Variant::Secondary,
        "Servers",
        Some(MenuAction::Navigate(MenuScreen::Servers)),
    )?;
    widgets::button(
        canvas,
        view,
        [midpoint + gap * 0.5, y, right, y + canvas.r(4.8)],
        Variant::Secondary,
        "Settings",
        Some(MenuAction::Navigate(MenuScreen::Settings)),
    )?;
    y += canvas.r(5.6);
    link(
        canvas,
        view,
        [x, y, midpoint - gap * 0.5, y + canvas.r(3.6)],
        "Realms",
        MenuAction::Navigate(MenuScreen::Social),
    )?;
    link(
        canvas,
        view,
        [midpoint + gap * 0.5, y, right, y + canvas.r(3.6)],
        "Marketplace",
        MenuAction::Store(crate::store::OPEN),
    )?;
    y += canvas.r(4.4);
    if compact {
        widgets::button(
            canvas,
            view,
            [x, y, right, y + canvas.r(4.8)],
            Variant::Neutral,
            "Dressing Room",
            Some(MenuAction::Navigate(MenuScreen::DressingRoom)),
        )?;
    }
    canvas.end_scroll(scroll, content_height - canvas.r(EDGE * 2.0))?;
    canvas.end_focus_region(content);
    Ok(())
}

fn character_panel(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
) -> Result<CharacterPreview, UiPresentationError> {
    widgets::panel(canvas, b)?;
    let region = canvas.begin_focus_region(focus::DETAIL, b, None, true)?;
    let pad = canvas.r(2.4);
    let x = b[0] + pad;
    let right = b[2] - pad;
    let top = b[1] + pad;
    canvas.text_line("YOUR CHARACTER", [x, top], right - x, HEADER5, TEXT)?;
    canvas.text_line(
        super::super::accounts::current_name(view),
        [x, top + canvas.r(HEADER5.line + 0.8)],
        right - x,
        CAPTION,
        TEXT_DIMMER,
    )?;
    let footer = b[3] - pad - canvas.r(4.8);
    widgets::button(
        canvas,
        view,
        [x, footer, right, footer + canvas.r(4.8)],
        Variant::Secondary,
        "Dressing Room",
        Some(MenuAction::Navigate(MenuScreen::DressingRoom)),
    )?;
    let hint = [x, footer - canvas.r(3.6), right, footer - canvas.r(1.2)];
    canvas.text_centred("Drag to rotate", hint, CAPTION, TEXT_DIMMER, false)?;
    let preview = [x, top + canvas.r(6.4), right, hint[1] - canvas.r(0.8)];
    canvas.end_focus_region(region);
    Ok(CharacterPreview {
        control: preview,
        clip: preview,
    })
}

fn link(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    label: &str,
    action: MenuAction,
) -> Result<(), UiPresentationError> {
    let state = canvas.interaction(view, Some(action));
    let motion = canvas.feedback(state, true, false, Kind::Surface);
    let down = canvas.r(0.2) * motion.press;
    let face = [bounds[0], bounds[1] + down, bounds[2], bounds[3]];
    let role = canvas.role(NEUTRAL80);
    canvas.fill(face, motion.color(role.fill, role.hovered, role.pressed))?;
    if motion.focus > 0.0 {
        canvas.frame(face, EDGE, opacity(OUTLINE, motion.focus))?;
    }
    canvas.text_centred(
        label,
        face,
        CAPTION,
        mix(TEXT_DIMMER, TEXT, motion.hover.max(motion.focus)),
        false,
    )?;
    canvas.hit(action, bounds)
}

fn live_event(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    event: &launcher::menu::LiveEventCard,
    translate: Translate<'_>,
) -> Result<(), UiPresentationError> {
    let label = translate(&event.button_text).map_or_else(
        || {
            if event.button_text.starts_with("gathering.") {
                "Live event"
            } else {
                event.button_text.as_str()
            }
            .to_owned()
        },
        |value| value.to_string(),
    );
    let badge = canvas
        .artwork
        .and_then(|art| art.get(&event.badge_path))
        .copied();
    let label_right = if badge.is_some() {
        b[2] - canvas.r(6.4)
    } else {
        b[2]
    };
    link(
        canvas,
        view,
        [b[0], b[1], label_right, b[3]],
        &label,
        MenuAction::OpenLiveEvent,
    )?;
    if let Some(badge) = badge {
        canvas.icon_ref(
            badge,
            [
                b[2] - canvas.r(5.6),
                b[1] + canvas.r(0.8),
                b[2] - canvas.r(0.8),
                b[3] - canvas.r(0.8),
            ],
        )?;
        canvas.hit(MenuAction::OpenLiveEvent, [label_right, b[1], b[2], b[3]])?;
    }
    Ok(())
}
