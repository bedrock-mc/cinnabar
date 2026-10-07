mod secondary;
mod servers;
mod status;

use status::{status_extent, status_paragraphs};

use ui::{SafeArea, TextLayoutCache, UiNode, UiRect};

use crate::menu::{MenuAction, MenuScreen, MenuServerCard, MenuView};

use super::{
    ContentArea, TextMetrics, UiPresentationError, bounded_visible_text,
    components::{button, card, solid, text},
    rect, text_visual_extent,
    tokens::*,
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
    content: ContentArea,
) -> Result<(), UiPresentationError> {
    match view.screen {
        // Without the engine the Marketplace has no launcher-style screen; show the start screen.
        MenuScreen::Home | MenuScreen::Store => home(
            view, nodes, hits, next_id, layouts, font, metrics, solid_page, content,
        ),
        MenuScreen::Play => play(
            view, nodes, hits, next_id, layouts, font, metrics, solid_page, content,
        ),
        MenuScreen::Social | MenuScreen::Friends | MenuScreen::Inbox => social(
            view, nodes, hits, next_id, layouts, font, metrics, solid_page, content,
        ),
        MenuScreen::Servers => servers::append(
            view, nodes, hits, next_id, layouts, font, metrics, solid_page, content,
        ),
        MenuScreen::Profile | MenuScreen::DressingRoom => secondary::profile(
            view, nodes, hits, next_id, layouts, font, metrics, solid_page, content,
        ),
        MenuScreen::Settings => secondary::settings(
            view, nodes, hits, next_id, layouts, font, metrics, solid_page, content,
        ),
        MenuScreen::AddServer => secondary::add_server(
            view, nodes, hits, next_id, layouts, font, metrics, solid_page, content,
        ),
        MenuScreen::Pause => secondary::pause(
            view, nodes, hits, next_id, layouts, font, metrics, solid_page, content,
        ),
        MenuScreen::Death => secondary::death(
            view, nodes, hits, next_id, layouts, font, metrics, solid_page, content,
        ),
    }
}

fn home(
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
    let gap = SPACE_MD;
    let right_width = if area.compact {
        0.0
    } else {
        (area.width * 0.36).clamp(300.0, 420.0)
    };
    let left_width = if area.compact {
        area.width
    } else {
        area.width - right_width - gap
    };
    panel(
        nodes, next_id, solid_page, area.left, area.top, left_width, 176.0,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        &format!("Welcome back, {}", view.display_name),
        [area.left + SPACE_LG, area.top + SPACE_LG],
        left_width - SPACE_LG * 2.0,
        TEXT,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Your worlds, friends, Realms, and servers live in one desktop-first launcher.",
        [area.left + SPACE_LG, area.top + 58.0],
        left_width - SPACE_LG * 2.0,
        MUTED,
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
        MenuAction::Navigate(MenuScreen::Play),
        usize::MAX,
        "Play",
        area.left + SPACE_LG,
        area.top + 108.0,
        190.0,
        TOUCH_CONTROL_HEIGHT,
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
        MenuAction::Navigate(MenuScreen::Servers),
        usize::MAX,
        "Browse servers",
        area.left + SPACE_LG + 202.0,
        area.top + 108.0,
        210.0_f32.min(left_width - 250.0).max(140.0),
        TOUCH_CONTROL_HEIGHT,
        SafeArea::ZERO,
    )?;

    let bottom = area.top + area.height;
    let feed_top = area.top + 192.0;
    let feed_heading = home_heading(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Jump back in",
        "Live destinations from your account",
        [area.left, feed_top],
        left_width,
        (bottom - feed_top - 106.0 - SPACE_MD).max(0.0),
    )?;
    let mut y = feed_top + feed_heading + if feed_heading > 0.0 { SPACE_MD } else { 0.0 };
    if let Some(friend) = view.friends.first().filter(|_| y + 78.0 <= bottom) {
        let server = MenuServerCard {
            name: friend.world_name.clone(),
            address: friend.gamertag.clone(),
            caption: format!("{} • joinable friend", friend.members),
            image_path: String::new(),
            icon: None,
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
            MenuAction::PlayFriend(0),
            usize::MAX,
            &server,
            view.friend_icon,
            [area.left, y],
            left_width,
            78.0,
            0.0,
        )?;
        y += 88.0;
    }
    if let Some(realm) = view.realms.first().filter(|_| y + 78.0 <= bottom) {
        let server = MenuServerCard {
            name: realm.name.clone(),
            address: realm.address.clone(),
            caption: format!("Realm • {}", realm.state),
            image_path: String::new(),
            icon: None,
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
            MenuAction::PlayRealm(0),
            usize::MAX,
            &server,
            view.realm_icon,
            [area.left, y],
            left_width,
            78.0,
            0.0,
        )?;
        y += 88.0;
    }
    let recovery_row = status_extent(
        layouts,
        font,
        metrics,
        "…",
        (left_width - SPACE_XL).max(1.0),
    )?
    .unwrap_or(f32::INFINITY);
    if view.friends.is_empty() && view.realms.is_empty() && bottom - y >= recovery_row + SPACE_XL {
        empty_state(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            if !view.catalog_loading && view.catalog_message.is_some() {
                "Account destinations unavailable"
            } else {
                "Nothing joinable right now"
            },
            catalog_status(view, "Friends and Realms will appear here when available."),
            [area.left, y],
            left_width,
            106.0_f32.min(bottom - y),
        )?;
    }

    if !area.compact {
        let right = area.left + left_width + gap;
        panel(
            nodes,
            next_id,
            solid_page,
            right,
            area.top,
            right_width,
            area.height,
        )?;
        let heading = home_heading(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            "Featured now",
            "Official Bedrock catalog",
            [right + SPACE_MD, area.top + SPACE_MD],
            right_width - SPACE_XL,
            (area.height - SPACE_MD * 2.0 - 120.0 - SPACE_MD).max(0.0),
        )?;
        let mut right_y =
            area.top + SPACE_MD + heading + if heading > 0.0 { SPACE_MD } else { 0.0 };
        for (index, server) in view.featured.iter().take(3).enumerate() {
            if right_y + 82.0 > bottom {
                break;
            }
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
                server.icon.or(view.featured_icon),
                [right + SPACE_MD, right_y],
                right_width - SPACE_XL,
                82.0,
                0.0,
            )?;
            right_y += 92.0;
        }
        let recovery_row = status_extent(
            layouts,
            font,
            metrics,
            "…",
            (right_width - SPACE_XL * 2.0).max(1.0),
        )?
        .unwrap_or(f32::INFINITY);
        if view.featured.is_empty() && bottom - right_y >= recovery_row + SPACE_XL {
            empty_state(
                nodes,
                next_id,
                layouts,
                font,
                metrics,
                solid_page,
                if view.catalog_loading {
                    "Loading featured servers…"
                } else {
                    "Featured servers unavailable"
                },
                catalog_status(view, "Social: Refresh to try again."),
                [right + SPACE_MD, right_y],
                right_width - SPACE_XL,
                120.0_f32.min(bottom - right_y),
            )?;
        }
    }
    Ok(())
}

fn play(
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
        "Play",
        "Choose a local world, friend world, Realm, or recent destination",
        [area.left, area.top],
        area.width,
    )?;
    let columns = if area.compact { 1 } else { 3 };
    let gap = SPACE_MD;
    let column_width = (area.width - gap * (columns - 1) as f32) / columns as f32;
    let top = area.top + 58.0;
    play_column(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Friends' worlds",
        0,
        [area.left, top],
        column_width,
        area.height - 64.0,
    )?;
    if columns > 1 {
        play_column(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            "Realms",
            1,
            [area.left + column_width + gap, top],
            column_width,
            area.height - 64.0,
        )?;
        play_column(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            "Recent servers",
            2,
            [area.left + (column_width + gap) * 2.0, top],
            column_width,
            area.height - 64.0,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn play_column(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    title: &str,
    kind: u8,
    position: [f32; 2],
    width: f32,
    height: f32,
) -> Result<(), UiPresentationError> {
    panel(
        nodes,
        next_id,
        solid_page,
        position[0],
        position[1],
        width,
        height,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        title,
        [position[0] + SPACE_MD, position[1] + SPACE_MD],
        width - SPACE_XL,
        TEXT,
    )?;
    let mut y = position[1] + 52.0;
    let mut count = 0usize;
    match kind {
        0 => {
            for (index, friend) in view.friends.iter().enumerate() {
                let server = MenuServerCard {
                    name: friend.world_name.clone(),
                    address: friend.gamertag.clone(),
                    caption: friend.members.clone(),
                    image_path: String::new(),
                    icon: None,
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
                    MenuAction::PlayFriend(index),
                    usize::MAX,
                    &server,
                    view.friend_icon,
                    [position[0] + SPACE_SM, y],
                    width - SPACE_SM * 2.0,
                    76.0,
                    0.0,
                )?;
                y += 84.0;
                count += 1;
            }
        }
        1 => {
            for (index, realm) in view.realms.iter().enumerate() {
                let server = MenuServerCard {
                    name: realm.name.clone(),
                    address: realm.address.clone(),
                    caption: realm.state.clone(),
                    image_path: String::new(),
                    icon: None,
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
                    MenuAction::PlayRealm(index),
                    usize::MAX,
                    &server,
                    view.realm_icon,
                    [position[0] + SPACE_SM, y],
                    width - SPACE_SM * 2.0,
                    76.0,
                    0.0,
                )?;
                y += 84.0;
                count += 1;
            }
        }
        _ => {
            let mut recent = view
                .servers
                .iter()
                .enumerate()
                .filter(|(_, server)| server.last_joined_unix > 0)
                .collect::<Vec<_>>();
            recent.sort_by_key(|(_, server)| std::cmp::Reverse(server.last_joined_unix));
            for (index, server) in recent {
                servers::saved_card(
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
                    [position[0] + SPACE_SM, y],
                    width - SPACE_SM * 2.0,
                    view.saved_icon,
                    false,
                )?;
                y += 84.0;
                count += 1;
            }
        }
    }
    if count == 0 {
        empty_state(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            match kind {
                0 => "No joinable friends",
                1 => "No Realms available",
                _ => "No recent servers",
            },
            if kind == 2 {
                "Servers you join will appear here."
            } else {
                catalog_status(view, "Account destinations will appear automatically.")
            },
            [position[0] + SPACE_SM, y],
            width - SPACE_SM * 2.0,
            122.0,
        )?;
    }
    Ok(())
}

fn social(
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
        "Friends & Social",
        "Joinable Xbox worlds are live; richer social actions stay clearly separated until their backend lands",
        [area.left, area.top],
        area.width - 160.0,
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
        MenuAction::RefreshCatalog,
        usize::MAX,
        if view.catalog_loading {
            "Refreshing…"
        } else {
            "Refresh"
        },
        area.left + area.width - 144.0,
        area.top,
        144.0,
        CONTROL_HEIGHT,
        SafeArea::ZERO,
    )?;
    let top = area.top + 62.0;
    let right_width = if area.compact {
        0.0
    } else {
        (area.width * 0.34).clamp(280.0, 370.0)
    };
    let left_width = if area.compact {
        area.width
    } else {
        area.width - right_width - SPACE_MD
    };
    panel(
        nodes,
        next_id,
        solid_page,
        area.left,
        top,
        left_width,
        area.height - 62.0,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        &format!("Joinable now  {}", view.friends.len()),
        [area.left + SPACE_MD, top + SPACE_MD],
        left_width - SPACE_XL,
        TEXT,
    )?;
    let mut y = top + 52.0;
    for (index, friend) in view.friends.iter().enumerate() {
        let server = MenuServerCard {
            name: friend.gamertag.clone(),
            address: friend.world_name.clone(),
            caption: format!("Online • {}", friend.members),
            image_path: String::new(),
            icon: None,
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
            MenuAction::PlayFriend(index),
            usize::MAX,
            &server,
            view.friend_icon,
            [area.left + SPACE_SM, y],
            left_width - SPACE_SM * 2.0,
            78.0,
            0.0,
        )?;
        status_marker(
            nodes,
            next_id,
            solid_page,
            area.left + left_width - 27.0,
            y + 18.0,
            SUCCESS,
        )?;
        y += 88.0;
    }
    if view.friends.is_empty() {
        empty_state(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            if view.catalog_loading {
                "Finding friends…"
            } else {
                "No joinable worlds"
            },
            catalog_status(
                view,
                "Friends appear when they make a Bedrock world joinable.",
            ),
            [area.left + SPACE_SM, y],
            left_width - SPACE_SM * 2.0,
            130.0,
        )?;
    }
    if !area.compact {
        let right = area.left + left_width + SPACE_MD;
        panel(nodes, next_id, solid_page, right, top, right_width, 182.0)?;
        section_heading(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            "Invites",
            "No pending game invites",
            [right + SPACE_MD, top + SPACE_MD],
            right_width - SPACE_XL,
        )?;
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            "Invite inbox and party APIs are not exposed by the current core yet.",
            [right + SPACE_MD, top + 88.0],
            right_width - SPACE_XL,
            SUBTLE,
        )?;
        panel(
            nodes,
            next_id,
            solid_page,
            right,
            top + 198.0,
            right_width,
            182.0,
        )?;
        section_heading(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            "Recent players",
            "Available after a gameplay session",
            [right + SPACE_MD, top + 214.0],
            right_width - SPACE_XL,
        )?;
    }
    Ok(())
}

fn section_heading(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    title: &str,
    subtitle: &str,
    position: [f32; 2],
    width: f32,
) -> Result<(), UiPresentationError> {
    text(
        nodes, next_id, layouts, font, metrics, solid_page, title, position, width, TEXT,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        subtitle,
        [position[0], position[1] + 27.0],
        width,
        MUTED,
    )
}

#[allow(clippy::too_many_arguments)]
fn empty_state(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    title: &str,
    body: &str,
    position: [f32; 2],
    width: f32,
    height: f32,
) -> Result<(), UiPresentationError> {
    solid(
        nodes,
        next_id,
        solid_page,
        rect(
            position[0],
            position[1],
            position[0] + width,
            position[1] + height,
        )?,
        PANEL_ALT,
    );
    let text_width = (width - SPACE_XL).max(1.0);
    let available = (height - SPACE_MD * 2.0).max(0.0);
    let paragraphs = status_paragraphs(
        layouts, font, metrics, title, body, text_width, available, SPACE_SM,
    )?;
    for (value, top, color) in [
        (paragraphs.title, SPACE_MD, TEXT),
        (paragraphs.body, SPACE_MD + paragraphs.body_top, MUTED),
    ] {
        if !value.is_empty() {
            text(
                nodes,
                next_id,
                layouts,
                font,
                metrics,
                solid_page,
                &value,
                [position[0] + SPACE_MD, position[1] + top],
                text_width,
                color,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn home_heading(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    title: &str,
    subtitle: &str,
    position: [f32; 2],
    width: f32,
    maximum_extent: f32,
) -> Result<f32, UiPresentationError> {
    let paragraphs = status_paragraphs(
        layouts,
        font,
        metrics,
        title,
        subtitle,
        width,
        maximum_extent,
        SPACE_SM,
    )?;
    for (value, offset, color) in [
        (paragraphs.title, 0.0, TEXT),
        (paragraphs.body, paragraphs.body_top, MUTED),
    ] {
        if !value.is_empty() {
            text(
                nodes,
                next_id,
                layouts,
                font,
                metrics,
                solid_page,
                &value,
                [position[0], position[1] + offset],
                width,
                color,
            )?;
        }
    }
    Ok(paragraphs.extent)
}

fn panel(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    solid_page: u16,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
) -> Result<(), UiPresentationError> {
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, top, left + width, top + height)?,
        BORDER,
    );
    solid(
        nodes,
        next_id,
        solid_page,
        rect(
            left + 1.0,
            top + 1.0,
            left + width - 1.0,
            top + height - 1.0,
        )?,
        PANEL,
    );
    Ok(())
}

fn status_marker(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    solid_page: u16,
    x: f32,
    y: f32,
    color: [u8; 4],
) -> Result<(), UiPresentationError> {
    solid(
        nodes,
        next_id,
        solid_page,
        rect(x, y, x + 9.0, y + 9.0)?,
        color,
    );
    Ok(())
}

fn catalog_status<'a>(view: &'a MenuView, fallback: &'a str) -> &'a str {
    view.catalog_message.as_deref().unwrap_or(fallback)
}
