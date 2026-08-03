//! Retained launcher/menu surfaces. Geometry is intentionally restrained: a
//! dark game-space canvas, a narrow action rail, and cards for server choices.
//! The visual language stays close to Java's desktop client while leaving the
//! Bedrock server catalog as a secondary, informational surface.

#![allow(clippy::too_many_arguments)]

use ui::{SafeArea, TextLayoutCache, UiNode, UiRect};

use crate::menu::{MenuAction, MenuField, MenuScreen, MenuView};

use super::{TextMetrics, UiPresentationError, bounded_visible_text, rect};

mod components;
use components::{button, card, field, solid, text};

const PANEL: [u8; 4] = [22, 28, 39, 244];
const PANEL_ALT: [u8; 4] = [31, 39, 53, 248];
const BUTTON: [u8; 4] = [42, 52, 68, 255];
const BUTTON_FOCUSED: [u8; 4] = [71, 123, 191, 255];
const BUTTON_HOVERED: [u8; 4] = [54, 73, 98, 255];
const BUTTON_PRESSED: [u8; 4] = [91, 145, 220, 255];
const TEXT: [u8; 4] = [235, 239, 247, 255];
const MUTED: [u8; 4] = [163, 174, 194, 255];
const ACCENT: [u8; 4] = [111, 211, 255, 255];

pub(super) fn append_menu_nodes(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    safe_area: SafeArea,
) -> Result<Vec<(MenuAction, UiRect)>, UiPresentationError> {
    if !view.visible {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    solid(
        nodes,
        next_id,
        solid_page,
        rect(0.0, 0.0, width, height)?,
        [8, 11, 17, 250],
    );
    let margin = 52.0_f32.min((width * 0.08).max(24.0));
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "CINNABAR",
        [margin, 42.0],
        260.0,
        ACCENT,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        match view.screen {
            MenuScreen::Main => "Bedrock, without the friction.",
            MenuScreen::Play => "Choose a world to join",
            MenuScreen::Realms => "Realms",
            MenuScreen::Friends => "Friends",
            MenuScreen::Settings => "Settings",
            MenuScreen::AddServer => "Add server",
            MenuScreen::Pause => "Game menu",
        },
        [margin, 76.0],
        (width - margin * 2.0).max(1.0),
        MUTED,
    )?;

    match view.screen {
        MenuScreen::Main => main_screen(
            view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, width, height,
            margin,
        )?,
        MenuScreen::Play => play_screen(
            view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, width, height,
            margin,
        )?,
        MenuScreen::Realms => realms_screen(
            view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, width, height,
            margin,
        )?,
        MenuScreen::Friends => friends_screen(
            view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, width, height,
            margin,
        )?,
        MenuScreen::Settings => settings_screen(
            view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, width, height,
            margin,
        )?,
        MenuScreen::AddServer => add_server_screen(
            view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, width, height,
            margin,
        )?,
        MenuScreen::Pause => pause_screen(
            view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, width, height,
            margin,
        )?,
    }
    Ok(hits
        .into_iter()
        .map(|(action, bounds)| {
            let translated = rect(
                bounds.min().x() + safe_area.left(),
                bounds.min().y() + safe_area.top(),
                bounds.max().x() + safe_area.left(),
                bounds.max().y() + safe_area.top(),
            )
            .expect("menu hit rectangles are finite");
            (action, translated)
        })
        .collect())
}

fn main_screen(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    margin: f32,
) -> Result<(), UiPresentationError> {
    let rail_width = 300.0_f32.min((width - margin * 2.0).max(1.0));
    let actions = [
        (MenuAction::MainPlay, "Play"),
        (MenuAction::MainRealms, "Realms"),
        (MenuAction::MainFriends, "Friends"),
        (MenuAction::MainSettings, "Settings"),
        (MenuAction::MainExit, "Exit"),
    ];
    for (index, (action, label)) in actions.into_iter().enumerate() {
        button(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            action,
            index,
            label,
            margin,
            138.0 + index as f32 * 56.0,
            rail_width,
            44.0,
            SafeArea::ZERO,
        )?;
    }
    let panel_left = (width - 430.0)
        .max(margin + rail_width + 36.0)
        .min(width - margin);
    let panel_width = (width - margin - panel_left).clamp(250.0, 430.0);
    let panel_top = 138.0;
    let panel_height = (height - panel_top - 42.0).max(190.0);
    solid(
        nodes,
        next_id,
        solid_page,
        rect(
            panel_left,
            panel_top,
            panel_left + panel_width,
            panel_top + panel_height,
        )?,
        PANEL,
    );
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Featured servers",
        [panel_left + 22.0, panel_top + 22.0],
        panel_width - 44.0,
        TEXT,
    )?;
    let mut y = panel_top + 58.0;
    if view.featured.is_empty() {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            view.catalog_message
                .as_deref()
                .unwrap_or("Loading featured servers…"),
            [panel_left + 22.0, y],
            panel_width - 44.0,
            MUTED,
        )?;
        y += 52.0;
    }
    for (index, server) in view.featured.iter().take(3).enumerate() {
        card(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::PlayFeatured(index),
            usize::MAX,
            server,
            view.featured_icon,
            [panel_left + 18.0, y],
            panel_width - 36.0,
            78.0,
        )?;
        y += 88.0;
    }
    if view.featured.len() > 3 {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            &format!("+ {} more in Play", view.featured.len() - 3),
            [panel_left + 22.0, y],
            panel_width - 44.0,
            MUTED,
        )?;
        y += 24.0;
    }
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Gatherings",
        [panel_left + 22.0, y + 4.0],
        panel_width - 44.0,
        ACCENT,
    )?;
    if let Some(server) = view.gatherings.first() {
        card(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::PlayGathering(0),
            usize::MAX,
            server,
            view.gathering_icon,
            [panel_left + 18.0, y + 34.0],
            panel_width - 36.0,
            78.0,
        )?;
    }
    Ok(())
}

fn play_screen(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    margin: f32,
) -> Result<(), UiPresentationError> {
    let left_width = (width * 0.54).clamp(300.0, 620.0);
    solid(
        nodes,
        next_id,
        solid_page,
        rect(
            margin,
            126.0,
            margin + left_width,
            (height - 42.0).max(220.0),
        )?,
        PANEL,
    );
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Saved servers",
        [margin + 22.0, 148.0],
        left_width - 44.0,
        TEXT,
    )?;
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::PlayAddServer,
        0,
        "+  Add server",
        margin + 18.0,
        182.0,
        left_width - 36.0,
        42.0,
        SafeArea::ZERO,
    )?;
    let mut y = 236.0;
    let mut focus_index = 1usize;
    if view.servers.is_empty() {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            "No saved servers yet. Add one here and it\nwill be available on every launch.",
            [margin + 24.0, y + 18.0],
            left_width - 48.0,
            MUTED,
        )?;
    }
    let max_saved = ((height - 282.0) / 108.0).floor().max(1.0) as usize;
    for (index, server) in view.servers.iter().take(max_saved).enumerate() {
        card(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::PlaySaved(index),
            focus_index,
            &crate::menu::MenuServerCard {
                name: server.name.clone(),
                address: server.address.clone(),
                caption: "Saved locally".to_owned(),
                icon: None,
            },
            view.saved_icon,
            [margin + 18.0, y],
            left_width - 36.0,
            96.0,
        )?;
        y += 108.0;
        focus_index += 1;
    }
    if view.servers.len() > max_saved {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            &format!("+ {} more saved servers", view.servers.len() - max_saved),
            [margin + 24.0, (height - 124.0).max(y)],
            left_width - 48.0,
            MUTED,
        )?;
    }
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::PlayBack,
        focus_index + view.featured.len() + view.gatherings.len(),
        "Back",
        margin + 18.0,
        height - 84.0,
        left_width - 36.0,
        42.0,
        SafeArea::ZERO,
    )?;

    let right_left = (margin + left_width + 22.0).min(width - margin - 250.0);
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Featured",
        [right_left, 148.0],
        width - right_left - margin,
        ACCENT,
    )?;
    let mut right_y = 182.0;
    if view.featured.is_empty() {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            view.catalog_message
                .as_deref()
                .unwrap_or("Loading featured servers…"),
            [right_left, right_y],
            width - right_left - margin,
            MUTED,
        )?;
        right_y += 52.0;
    }
    for (index, server) in view.featured.iter().take(3).enumerate() {
        card(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::PlayFeatured(index),
            focus_index + index,
            server,
            view.featured_icon,
            [right_left, right_y],
            width - right_left - margin,
            78.0,
        )?;
        right_y += 88.0;
    }
    if view.featured.len() > 3 {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            &format!("+ {} more in account", view.featured.len() - 3),
            [right_left, right_y],
            width - right_left - margin,
            MUTED,
        )?;
        right_y += 24.0;
    }
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Gatherings",
        [right_left, right_y + 8.0],
        width - right_left - margin,
        ACCENT,
    )?;
    if let Some(server) = view.gatherings.first() {
        card(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::PlayGathering(0),
            focus_index + view.featured.len(),
            server,
            view.gathering_icon,
            [right_left, right_y + 42.0],
            width - right_left - margin,
            78.0,
        )?;
    }
    Ok(())
}

fn realms_screen(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    margin: f32,
) -> Result<(), UiPresentationError> {
    let panel_width = (width.min(980.0) - margin * 2.0).max(1.0);
    let left = (width - panel_width) * 0.5;
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, 126.0, left + panel_width, (height - 42.0).max(300.0))?,
        PANEL,
    );
    let y = 166.0;
    if view.realms.is_empty() {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            view.catalog_message
                .as_deref()
                .unwrap_or("Loading your Realms…"),
            [left + 28.0, y],
            panel_width - 56.0,
            MUTED,
        )?;
    } else {
        let columns = 3usize;
        let gap = 10.0;
        let card_width =
            ((panel_width - 36.0 - gap * (columns - 1) as f32) / columns as f32).max(1.0);
        let max_rows = ((height - 250.0) / 108.0).floor().max(1.0) as usize;
        let max_cards = columns * max_rows;
        for (index, realm) in view.realms.iter().take(max_cards).enumerate() {
            let column = index % columns;
            let row = index / columns;
            card(
                view,
                nodes,
                hits,
                next_id,
                layouts,
                font,
                metrics,
                solid_page,
                MenuAction::PlayRealm(index),
                index,
                &crate::menu::MenuServerCard {
                    name: realm.name.clone(),
                    address: if realm.address.is_empty() {
                        realm.state.clone()
                    } else {
                        realm.address.clone()
                    },
                    caption: if realm.state.is_empty() {
                        "Realm".to_owned()
                    } else {
                        realm.state.clone()
                    },
                    icon: None,
                },
                view.realm_icon,
                [
                    left + 18.0 + column as f32 * (card_width + gap),
                    y + row as f32 * 108.0,
                ],
                card_width,
                96.0,
            )?;
        }
        if view.realms.len() > max_cards {
            text(
                nodes,
                next_id,
                layouts,
                font,
                metrics,
                solid_page,
                &format!("+ {} more realms available", view.realms.len() - max_cards),
                [left + 28.0, height - 136.0],
                panel_width - 56.0,
                MUTED,
            )?;
        }
    }
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::RealmsBack,
        view.realms.len().min(12),
        "Back",
        left + 28.0,
        height - 104.0,
        180.0,
        42.0,
        SafeArea::ZERO,
    )
}

fn friends_screen(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    margin: f32,
) -> Result<(), UiPresentationError> {
    let panel_width = (width.min(980.0) - margin * 2.0).max(1.0);
    let left = (width - panel_width) * 0.5;
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, 126.0, left + panel_width, (height - 42.0).max(300.0))?,
        PANEL,
    );
    let y = 166.0;
    if view.friends.is_empty() {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            view.catalog_message
                .as_deref()
                .unwrap_or("Loading joinable friend worlds…"),
            [left + 28.0, y],
            panel_width - 56.0,
            MUTED,
        )?;
    } else {
        let columns = 3usize;
        let gap = 10.0;
        let card_width =
            ((panel_width - 36.0 - gap * (columns - 1) as f32) / columns as f32).max(1.0);
        let max_rows = ((height - 250.0) / 108.0).floor().max(1.0) as usize;
        let max_cards = columns * max_rows;
        for (index, friend) in view.friends.iter().take(max_cards).enumerate() {
            let column = index % columns;
            let row = index / columns;
            card(
                view,
                nodes,
                hits,
                next_id,
                layouts,
                font,
                metrics,
                solid_page,
                MenuAction::PlayFriend(index),
                index,
                &crate::menu::MenuServerCard {
                    name: friend.world_name.clone(),
                    address: friend.gamertag.clone(),
                    caption: friend.members.clone(),
                    icon: None,
                },
                view.friend_icon,
                [
                    left + 18.0 + column as f32 * (card_width + gap),
                    y + row as f32 * 108.0,
                ],
                card_width,
                96.0,
            )?;
        }
        if view.friends.len() > max_cards {
            text(
                nodes,
                next_id,
                layouts,
                font,
                metrics,
                solid_page,
                &format!(
                    "+ {} more friends available",
                    view.friends.len() - max_cards
                ),
                [left + 28.0, height - 136.0],
                panel_width - 56.0,
                MUTED,
            )?;
        }
    }
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::FriendsBack,
        view.friends.len().min(12),
        "Back",
        left + 28.0,
        height - 104.0,
        180.0,
        42.0,
        SafeArea::ZERO,
    )
}

fn settings_screen(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    margin: f32,
) -> Result<(), UiPresentationError> {
    let panel_width = width.min(760.0) - margin * 2.0;
    let left = (width - panel_width) * 0.5;
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, 126.0, left + panel_width, (height - 42.0).max(300.0))?,
        PANEL,
    );
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Interface scale",
        [left + 28.0, 156.0],
        panel_width - 56.0,
        TEXT,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Java-style GUI scaling keeps chat, HUD, and menus aligned.",
        [left + 28.0, 184.0],
        panel_width - 56.0,
        MUTED,
    )?;
    for (offset, scale) in [1u8, 2, 3, 4].into_iter().enumerate() {
        button(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::SettingsScale(scale),
            offset,
            format!("Scale {scale}").as_str(),
            left + 28.0 + offset as f32 * 108.0,
            236.0,
            96.0,
            42.0,
            SafeArea::ZERO,
        )?;
    }
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        &format!("Current scale: {}", view.gui_scale),
        [left + 28.0, 300.0],
        panel_width - 56.0,
        ACCENT,
    )?;
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::SettingsBack,
        4,
        "Back",
        left + 28.0,
        (height - 104.0).max(348.0),
        180.0,
        42.0,
        SafeArea::ZERO,
    )
}

fn add_server_screen(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    margin: f32,
) -> Result<(), UiPresentationError> {
    let panel_width = width.min(700.0) - margin * 2.0;
    let left = (width - panel_width) * 0.5;
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, 126.0, left + panel_width, (height - 42.0).max(360.0))?,
        PANEL,
    );
    field(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddName,
        0,
        MenuField::Name,
        "Name",
        &view.name,
        [left + 28.0, 164.0],
        panel_width - 56.0,
    )?;
    field(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddAddress,
        1,
        MenuField::Address,
        "Address",
        &view.address,
        [left + 28.0, 252.0],
        panel_width - 56.0,
    )?;
    if let Some(message) = &view.message {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            message,
            [left + 28.0, 344.0],
            panel_width - 56.0,
            [255, 191, 104, 255],
        )?;
    }
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddSave,
        2,
        "Save",
        left + 28.0,
        (height - 108.0).max(390.0),
        130.0,
        42.0,
        SafeArea::ZERO,
    )?;
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddSaveConnect,
        3,
        "Save & play",
        left + 172.0,
        (height - 108.0).max(390.0),
        150.0,
        42.0,
        SafeArea::ZERO,
    )?;
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddBack,
        4,
        "Cancel",
        left + panel_width - 158.0,
        (height - 108.0).max(390.0),
        130.0,
        42.0,
        SafeArea::ZERO,
    )
}

fn pause_screen(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    margin: f32,
) -> Result<(), UiPresentationError> {
    let left = (width - 300.0).max(margin);
    let actions = [
        (MenuAction::PauseResume, "Back to game"),
        (MenuAction::PauseSettings, "Settings"),
        (MenuAction::PauseDisconnect, "Disconnect"),
    ];
    for (index, (action, label)) in actions.into_iter().enumerate() {
        button(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            action,
            index,
            label,
            left,
            (height * 0.5 - 60.0) + index as f32 * 54.0,
            300.0_f32.min(width - left - margin),
            44.0,
            SafeArea::ZERO,
        )?;
    }
    Ok(())
}
