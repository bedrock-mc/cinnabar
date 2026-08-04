use ui::{SafeArea, TextLayoutCache, UiNode, UiRect};

use crate::{
    menu::{MenuAction, MenuServerCard, MenuServerTab, MenuView, SavedServer},
    ui_runtime::presentation::IconRef,
};

use super::{
    super::{
        components::{button, card, solid},
        rect,
        tokens::*,
    },
    ContentArea, TextMetrics, UiPresentationError, catalog_status, empty_state, panel,
    section_heading,
};

pub(super) fn append(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    area: ContentArea,
) -> Result<(), UiPresentationError> {
    section_heading(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Servers",
        "Official discovery, Gatherings, favorites, recent, and saved servers",
        [area.left, area.top],
        area.width - 170.0,
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
        usize::MAX,
        "Add server",
        area.left + area.width - 156.0,
        area.top,
        156.0,
        CONTROL_HEIGHT,
        SafeArea::ZERO,
    )?;
    let tab_top = area.top + 58.0;
    let tab_gap = SPACE_XS;
    let tab_width = ((area.width - tab_gap * 3.0) / 4.0).max(1.0);
    for (index, (tab, label)) in [
        (MenuServerTab::Featured, "Featured"),
        (MenuServerTab::Favorites, "Favorites"),
        (MenuServerTab::Recent, "Recent"),
        (MenuServerTab::Saved, "Saved"),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::SelectServerTab(tab),
            usize::MAX,
            label,
            area.left + index as f32 * (tab_width + tab_gap),
            tab_top,
            tab_width,
            CONTROL_HEIGHT,
            SafeArea::ZERO,
        )?;
        if view.server_tab == tab {
            solid(
                nodes,
                next_id,
                solid_page,
                rect(
                    area.left + index as f32 * (tab_width + tab_gap),
                    tab_top + CONTROL_HEIGHT - 3.0,
                    area.left + index as f32 * (tab_width + tab_gap) + tab_width,
                    tab_top + CONTROL_HEIGHT,
                )?,
                ACCENT,
            );
        }
    }
    let list_top = tab_top + CONTROL_HEIGHT + SPACE_MD;
    let list_height = area.height - (list_top - area.top);
    panel(
        nodes,
        next_id,
        solid_page,
        area.left,
        list_top,
        area.width,
        list_height,
    )?;
    match view.server_tab {
        MenuServerTab::Featured => featured_servers(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            [area.left + SPACE_MD, list_top + SPACE_MD],
            area.width - SPACE_XL,
            list_height - SPACE_XL,
            area.compact,
        ),
        MenuServerTab::Favorites => saved_server_list(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            [area.left + SPACE_MD, list_top + SPACE_MD],
            area.width - SPACE_XL,
            list_height - SPACE_XL,
            |server| server.favorite,
            "No favorite servers",
            "Mark a saved server as a favorite from its row.",
        ),
        MenuServerTab::Recent => saved_server_list(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            [area.left + SPACE_MD, list_top + SPACE_MD],
            area.width - SPACE_XL,
            list_height - SPACE_XL,
            |server| server.last_joined_unix > 0,
            "No recent servers",
            "A server appears here after you join it.",
        ),
        MenuServerTab::Saved => saved_server_list(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            [area.left + SPACE_MD, list_top + SPACE_MD],
            area.width - SPACE_XL,
            list_height - SPACE_XL,
            |_| true,
            "No saved servers",
            "Add a Java-style direct server entry to keep it here.",
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn featured_servers(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    position: [f32; 2],
    width: f32,
    height: f32,
    compact: bool,
) -> Result<(), UiPresentationError> {
    if view.featured.is_empty() && view.gatherings.is_empty() {
        return empty_state(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            if view.catalog_loading {
                "Loading catalog…"
            } else {
                "Catalog unavailable"
            },
            catalog_status(view, "Use Refresh from Social or reopen the launcher."),
            position,
            width,
            150.0,
        );
    }
    let columns = if compact || width < 700.0 { 1 } else { 2 };
    let gap = SPACE_SM;
    let card_width = (width - gap * (columns - 1) as f32) / columns as f32;
    let max_rows = (height / 92.0).floor().max(1.0) as usize;
    let max_cards = columns * max_rows;
    let entries = view
        .featured
        .iter()
        .enumerate()
        .map(|(index, server)| {
            (
                MenuAction::PlayFeatured(index),
                server,
                view.featured_icon,
                "Featured",
            )
        })
        .chain(view.gatherings.iter().enumerate().map(|(index, server)| {
            (
                MenuAction::PlayGathering(index),
                server,
                view.gathering_icon,
                "Gathering",
            )
        }));
    for (display_index, (action, server, fallback, category)) in entries.take(max_cards).enumerate()
    {
        let column = display_index % columns;
        let row = display_index / columns;
        let mut card_model = server.clone();
        card_model.caption = if server.caption.is_empty() {
            category.to_owned()
        } else {
            format!("{category} • {}", server.caption)
        };
        card(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            action,
            usize::MAX,
            &card_model,
            server.icon.or(fallback),
            [
                position[0] + column as f32 * (card_width + gap),
                position[1] + row as f32 * 92.0,
            ],
            card_width,
            82.0,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn saved_server_list<F: Fn(&SavedServer) -> bool>(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    position: [f32; 2],
    width: f32,
    height: f32,
    predicate: F,
    empty_title: &str,
    empty_body: &str,
) -> Result<(), UiPresentationError> {
    let mut entries = view
        .servers
        .iter()
        .enumerate()
        .filter(|(_, server)| predicate(server))
        .collect::<Vec<_>>();
    if view.server_tab == MenuServerTab::Recent {
        entries.sort_by_key(|(_, server)| std::cmp::Reverse(server.last_joined_unix));
    }
    if entries.is_empty() {
        return empty_state(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            empty_title,
            empty_body,
            position,
            width,
            150.0,
        );
    }
    let max_cards = (height / 92.0).floor().max(1.0) as usize;
    for (row, (index, server)) in entries.into_iter().take(max_cards).enumerate() {
        saved_card(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            index,
            server,
            [position[0], position[1] + row as f32 * 92.0],
            width,
            view.saved_icon,
            true,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn saved_card(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    index: usize,
    server: &SavedServer,
    position: [f32; 2],
    width: f32,
    icon: Option<IconRef>,
    actions: bool,
) -> Result<(), UiPresentationError> {
    let card_model = MenuServerCard {
        name: server.name.clone(),
        address: server.address.clone(),
        caption: if server.favorite {
            "Favorite • saved locally".to_owned()
        } else {
            "Saved locally".to_owned()
        },
        image_path: String::new(),
        icon: None,
    };
    let action_width = if actions {
        208.0_f32.min(width * 0.38)
    } else {
        0.0
    };
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
        usize::MAX,
        &card_model,
        icon,
        position,
        width,
        82.0,
    )?;
    if actions {
        let small_width = (action_width - SPACE_XS) * 0.5;
        button(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::ToggleFavorite(index),
            usize::MAX,
            if server.favorite {
                "Unfavorite"
            } else {
                "Favorite"
            },
            position[0] + width - action_width,
            position[1] + 18.0,
            small_width,
            CONTROL_HEIGHT,
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
            MenuAction::RemoveSavedDialog(index),
            usize::MAX,
            "Remove",
            position[0] + width - small_width,
            position[1] + 18.0,
            small_width,
            CONTROL_HEIGHT,
            SafeArea::ZERO,
        )?;
    }
    Ok(())
}
