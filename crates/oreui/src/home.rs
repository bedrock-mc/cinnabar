//! The owner's home layout keeps play, identity and utility actions together.

mod footer;
mod layout;
#[cfg(test)]
mod tests;

use super::{
    CharacterPreview, focus,
    motion::{Kind, mix, opacity},
    paint::{Bounds, Canvas},
    theme::{CAPTION, EDGE, HEADER5, NEUTRAL80, OUTLINE, TEXT, TEXT_DIMMER, TEXT_DIMMEST},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuScreen, MenuView};
use ui::IconRef;
use ui_engine::engine::screen::PaintError;
use ui_engine::engine::screen::Translate;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    portrait: Option<IconRef>,
    translate: Translate<'_>,
) -> Result<Option<CharacterPreview>, PaintError> {
    let layout = layout::Layout::new(
        canvas.rem,
        size,
        canvas.title_artwork,
        view.feeds.home.live_event.is_some(),
        super::updates::available(view),
    );
    let preview = canvas.with_scale(layout.scale, |canvas| {
        draw_layout(canvas, view, size, portrait, translate, layout)
    })?;
    super::updates::notice(canvas, view, size)?;
    Ok(preview)
}

/// Draws the home group at the fitted scale.
fn draw_layout(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    portrait: Option<IconRef>,
    translate: Translate<'_>,
    layout: layout::Layout,
) -> Result<Option<CharacterPreview>, PaintError> {
    canvas.overlay(size, [0, 0, 0, 96])?;
    let layout::Layout {
        width,
        height,
        outside,
        header_height,
        gap,
        footer_height,
        logo_width,
        logo_height,
        compact,
        compact_footer,
        ..
    } = layout;
    let left = (size[0] - width) * 0.5;
    let group_top = (size[1] - header_height - height - outside) * 0.5 + header_height;
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
        [
            left,
            group_top,
            left + width,
            size[1] - group_top + header_height,
        ],
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
        size[1] - group_top + header_height,
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
) -> Result<(), PaintError> {
    panel(canvas, bounds)?;
    let pad = layout::action_padding(canvas.rem, bounds[2] - bounds[0], compact);
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
    let clip = canvas.begin_clip(viewport)?;
    let event_height = if view.feeds.home.live_event.is_some() {
        layout::LIVE_EVENT_HEIGHT
    } else {
        0.0
    };
    let x = bounds[0] + pad;
    let right = bounds[2] - pad;
    let mut y = bounds[1] + pad + canvas.r(event_height);
    if let Some(event) = &view.feeds.home.live_event {
        let b = [
            x,
            y - canvas.r(layout::LIVE_EVENT_HEIGHT),
            right,
            y - canvas.r(1.2),
        ];
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
        [x, y, right, y + canvas.r(3.6)],
        "Realms",
        MenuAction::Navigate(MenuScreen::Social),
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
    canvas.end_clip(clip);
    canvas.end_focus_region(content);
    Ok(())
}

/// Draws a lighter home surface while leaving the panorama visible beneath it.
fn panel(canvas: &mut Canvas<'_>, bounds: Bounds) -> Result<(), PaintError> {
    let role = canvas.role(NEUTRAL80);
    let mut fill = role.fill;
    fill[3] = 200;
    canvas.fill(bounds, fill)?;
    let mut border = role.border;
    border[3] = 160;
    canvas.frame(bounds, EDGE, border)
}

/// Draws the player name, skin preview and full-width Dressing Room action.
fn character_panel(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
) -> Result<CharacterPreview, PaintError> {
    panel(canvas, b)?;
    let region = canvas.begin_focus_region(focus::DETAIL, b, None, true)?;
    let pad = canvas.r(layout::PANEL_PADDING);
    let x = b[0] + pad;
    let right = b[2] - pad;
    let top = b[1] + pad;
    canvas.text_line("YOUR CHARACTER", [x, top], right - x, HEADER5, TEXT)?;
    canvas.text_line(
        launcher::menu::profile::current_name(view),
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

/// Draws a text link with interaction feedback and no idle Realms backing.
fn link(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    label: &str,
    action: MenuAction,
) -> Result<(), PaintError> {
    let state = canvas.interaction(view, Some(action));
    let motion = canvas.feedback(state, true, false, Kind::Surface);
    let down = canvas.r(0.2) * motion.press;
    let face = [bounds[0], bounds[1] + down, bounds[2], bounds[3]];
    let role = canvas.role(NEUTRAL80);
    let mut fill = role.fill;
    if action == MenuAction::Navigate(MenuScreen::Social) {
        fill[3] = 0;
    }
    canvas.fill(face, motion.color(fill, role.hovered, role.pressed))?;
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
) -> Result<(), PaintError> {
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
