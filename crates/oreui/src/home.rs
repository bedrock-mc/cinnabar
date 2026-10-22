//! The home menu centers its main actions over the panorama.

mod footer;
mod layout;
#[cfg(test)]
mod tests;

use super::{
    CharacterPreview, focus,
    motion::{Kind, mix, opacity},
    paint::{Bounds, Canvas},
    theme::{CAPTION, EDGE, NEUTRAL80, OUTLINE, TEXT, TEXT_DIMMER, TEXT_DIMMEST},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuScreen, MenuView};
use ui::IconRef;
use ui_engine::engine::screen::{PaintError, Translate};

const ACTIONS: [(&str, MenuScreen); 4] = [
    ("Play", MenuScreen::Play),
    ("Settings", MenuScreen::Settings),
    ("Realms", MenuScreen::Social),
    ("Servers", MenuScreen::Servers),
];

/// Draws the title, a uniform action column and independent profile controls.
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
        draw_layout(canvas, view, size, portrait, translate, &layout)
    })?;
    super::updates::notice(canvas, view, size)?;
    Ok(preview)
}

/// Uses the same fitted GUI scale for labels, painted rectangles and hit regions.
fn draw_layout(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    portrait: Option<IconRef>,
    translate: Translate<'_>,
    layout: &layout::Layout,
) -> Result<Option<CharacterPreview>, PaintError> {
    canvas.overlay(size, [0, 0, 0, 64])?;
    if let Some(icon) = canvas.title_artwork {
        canvas.icon_ref(icon, layout.logo)?;
    } else {
        canvas.text_centred(
            launcher::PRODUCT_NAME,
            layout.logo,
            super::theme::HEADER3,
            TEXT,
            false,
        )?;
    }
    let screen =
        canvas.begin_focus_region(focus::SCREEN, [0.0, 0.0, size[0], size[1]], None, true)?;
    canvas.focus_delegate(Some(MenuAction::Navigate(MenuScreen::Play)), None);
    let content = canvas.begin_focus_region(
        focus::CONTENT,
        layout.actions,
        Some(launcher::menu::view::SettingsFocusAxis::Vertical),
        true,
    )?;
    canvas.focus_delegate(Some(MenuAction::Navigate(MenuScreen::Play)), None);
    if let Some(event) = &view.feeds.home.live_event {
        live_event(
            canvas,
            view,
            [
                layout.actions[0],
                layout.actions[1] - layout.gui * layout::LIVE_EVENT_HEIGHT,
                layout.actions[2],
                layout.actions[1] - layout.gui * 4.0,
            ],
            event,
            translate,
        )?;
    }
    for (index, (label, route)) in ACTIONS
        .into_iter()
        .chain(
            layout
                .compact
                .then_some(("Dressing Room", MenuScreen::DressingRoom)),
        )
        .enumerate()
    {
        let y = layout.actions[1] + index as f32 * layout::ROW_HEIGHT * layout.gui;
        widgets::button(
            canvas,
            view,
            [
                layout.actions[0] + layout.gui,
                y + layout.gui,
                layout.actions[2] - layout.gui,
                y + (layout::ROW_HEIGHT - 1.0) * layout.gui,
            ],
            Variant::Secondary,
            label,
            Some(MenuAction::Navigate(route)),
        )?;
    }
    canvas.end_focus_region(content);
    let preview = if layout.compact {
        None
    } else {
        Some(character(canvas, view, size, layout)?)
    };
    footer::draw(canvas, view, layout.footer, layout.compact_footer, portrait)?;
    let app = format!("{} {}", launcher::PRODUCT_NAME, launcher::PRODUCT_VERSION);
    let game = format!(
        "Minecraft {} / Protocol {}",
        protocol::GAME_VERSION,
        protocol::PROTOCOL_VERSION
    );
    canvas.text_centred(
        &format!("{app}  /  {game}"),
        layout.version,
        super::theme::Type {
            size: 1.1,
            line: 1.6,
            ..CAPTION
        },
        TEXT_DIMMEST,
        false,
    )?;
    canvas.end_focus_region(screen);
    Ok(preview)
}

/// Places the identity and wardrobe beside the main column without a backing card.
fn character(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    layout: &layout::Layout,
) -> Result<CharacterPreview, PaintError> {
    let gui = layout.gui;
    let center = size[0] * layout::CHARACTER_ANCHOR;
    let width = (layout::CHARACTER_WIDTH * gui).min((size[0] - layout.actions[2]) * 0.8);
    let bottom = layout.actions[3].min(layout.footer[1] - gui * 28.0);
    let wardrobe = [
        center - width * 0.5,
        bottom,
        center + width * 0.5,
        bottom + gui * 24.0,
    ];
    let top = (bottom - gui * 140.0).max(layout.logo[3] + gui * 16.0);
    let region = canvas.begin_focus_region(
        focus::DETAIL,
        [wardrobe[0], top - gui * 12.0, wardrobe[2], wardrobe[3]],
        None,
        true,
    )?;
    canvas.text_centred(
        launcher::menu::profile::current_name(view),
        [
            layout.actions[2] + gui * layout::CHARACTER_GAP,
            top - gui * 12.0,
            size[0] - gui * 4.0,
            top - gui * 2.0,
        ],
        CAPTION,
        TEXT,
        false,
    )?;
    widgets::button(
        canvas,
        view,
        wardrobe,
        Variant::Secondary,
        "Dressing Room",
        Some(MenuAction::Navigate(MenuScreen::DressingRoom)),
    )?;
    canvas.end_focus_region(region);
    let preview = [wardrobe[0], top, wardrobe[2], bottom - gui * 6.0];
    Ok(CharacterPreview {
        control: preview,
        clip: preview,
    })
}

/// Draws a lightweight event link with the menu's focus and press feedback.
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
    canvas.fill(face, motion.color([0, 0, 0, 0], role.hovered, role.pressed))?;
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
