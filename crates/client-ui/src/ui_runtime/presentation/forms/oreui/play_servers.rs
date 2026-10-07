//! The play route's Servers tab (classic layout): the side menu in four of
//! twelve columns (Add server, featured experiences then custom servers) and
//! the selected server's details in eight (10:3 banner with ping and players,
//! name with the hero Play button, then description, activities and news for
//! an experience, or edit and remove for a saved server).

use std::collections::HashMap;

mod details;
pub(super) mod drag;
mod list;
use details::{details, saved_details};

#[cfg(test)]
mod catalog_tests;
#[cfg(test)]
mod list_tests;

use super::super::super::{IconRef, UiPresentationError};
use super::super::play_screen::play_featured;
use super::grid::{Grid, space};
use super::icons::{self, Icon};
use super::paint::{Bounds, Canvas};
use super::theme::{BODY, CAPTION, NEUTRAL80, NEUTRAL100, TEXT, TEXT_DIMMER};
use super::widgets::{Variant, button, divider, side_menu};
use crate::menu::{MenuAction, MenuServerCard, MenuView, PingInfo, pingable};
use launcher::menu::server_list::ServerGroup;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    grid: &Grid,
    body: Bounds,
    images: &HashMap<String, IconRef>,
) -> Result<(), UiPresentationError> {
    canvas.settings_scrollbars = true;
    let (menu_span, details_span) = if grid.narrow {
        ((0, 3), (3, 5))
    } else {
        ((0, 4), (4, 8))
    };
    let [menu_left, menu_right] = grid.span(menu_span.0, menu_span.1);
    let row_right = menu_right - canvas.r(1.6);
    side_menu(canvas, [menu_left, body[1], row_right, body[3]])?;
    let pad = canvas.r(1.6);
    let list = canvas.begin_scroll(
        "servers.side_menu",
        [menu_left, body[1], menu_right, body[3]],
    )?;
    let top = body[1] - list.offset;
    let mut y = top + space(canvas, 2);
    let add_height = canvas.r(4.4);
    let filter_width = add_height;
    let filter_left = row_right - pad - filter_width;
    add_server(
        canvas,
        view,
        [
            menu_left + pad,
            y,
            filter_left - canvas.r(0.8),
            y + add_height,
        ],
    )?;
    filter_button(
        canvas,
        view,
        [filter_left, y, row_right - pad, y + add_height],
    )?;
    y += add_height;
    let selected = selection(view);
    if let Some(transitions) = canvas.transitions.as_deref_mut() {
        transitions.begin_servers(match selected {
            Some(Selection::Featured(index)) => Some(index),
            _ => None,
        });
    }
    y = list::draw(
        canvas,
        view,
        [menu_left, row_right],
        [body[1], body[3]],
        y,
        selected,
        images,
    )?;
    canvas.end_scroll(list, y + space(canvas, 2) - top)?;
    let [left, right] = grid.span(details_span.0, details_span.1);
    let panel = [left, body[1], right, body[3]];
    let entrance = selected
        .map(|selection| match selection {
            Selection::Featured(index) => super::motion::Surface::ServerDetails(index, false),
            Selection::Saved(index) => super::motion::Surface::ServerDetails(index, true),
        })
        .map(|surface| canvas.begin_entrance(surface));
    match selected {
        Some(Selection::Featured(index)) => {
            details(canvas, view, &view.featured[index], index, panel, images)?
        }
        Some(Selection::Saved(index)) => saved_details(canvas, view, index, panel)?,
        None => {}
    }
    if let Some(entrance) = entrance {
        canvas.end_entrance(entrance, [body[2], body[3]])?;
    }
    Ok(())
}

/// The section picker uses the existing filter icon on the same elevated button face.
fn filter_button(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
) -> Result<(), UiPresentationError> {
    let action = Some(MenuAction::OpenServerFilter);
    button(canvas, view, bounds, Variant::Secondary, "", action)?;
    let state = canvas.interaction(view, action);
    let motion = canvas.feedback(state, true, false, super::motion::Kind::Button);
    let [width, height] = Icon::Filter.texels();
    let texel = canvas.r(super::theme::EDGE);
    let at = [
        (bounds[0] + bounds[2] - width as f32 * texel) * 0.5,
        (bounds[1] + bounds[3] - canvas.r(0.4) - height as f32 * texel) * 0.5
            + canvas.r(0.4) * motion.press,
    ];
    let text = canvas.role(super::theme::SECONDARY).text;
    icons::draw(canvas, Icon::Filter, at, text)
}

fn add_server(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
) -> Result<(), UiPresentationError> {
    let action = Some(MenuAction::PlayAddServer);
    button(canvas, view, b, Variant::Secondary, "", action)?;
    let state = canvas.interaction(view, action);
    let motion = canvas.feedback(state, true, false, super::motion::Kind::Button);
    let side = canvas.r(2.4);
    let width = canvas.measure("Add server", BODY)?;
    let start = (b[0] + b[2] - width - side - canvas.r(0.4)) * 0.5;
    let top = b[1] + canvas.r(0.4) * motion.press;
    let bottom = b[3] - canvas.r(0.4) * (1.0 - motion.press);
    let icon_top = (top + bottom - side) * 0.5;
    let icon = [start, icon_top, start + side, icon_top + side];
    let text = canvas.role(super::theme::SECONDARY).text;
    if !canvas.masked_sprite(
        crate::ui_runtime::oreui_assets::SERVER_ADD_IMAGE,
        icon,
        text,
    )? {
        canvas.text_centred("+", icon, BODY, text, false)?;
    }
    canvas.text_line_vertically_centred(
        "Add server",
        [
            start + side + canvas.r(0.4),
            top,
            start + side + canvas.r(0.4) + width + 1.0,
            bottom,
        ],
        BODY,
        text,
    )
}

/// Experience grouping is shared by visible rows and the selected details.
fn experience_group(view: &MenuView, server: &MenuServerCard) -> ServerGroup {
    if view
        .feeds
        .details
        .get(&server.address)
        .is_some_and(|d| d.group == "creator")
    {
        ServerGroup::Creator
    } else {
        ServerGroup::Featured
    }
}

fn group_entries(view: &MenuView, group: &str) -> Vec<usize> {
    let group = if group == "creator" {
        ServerGroup::Creator
    } else {
        ServerGroup::Featured
    };
    view.featured
        .iter()
        .enumerate()
        .filter_map(|(index, server)| (experience_group(view, server) == group).then_some(index))
        .collect()
}

fn server_caption(view: &MenuView, server: &MenuServerCard) -> String {
    let text = view
        .feeds
        .pings
        .get(&server.address)
        .filter(|p| !p.motd.is_empty())
        .map_or(server.caption.as_str(), |p| p.motd.as_str());
    let mut chars = text.chars();
    let mut clean = String::with_capacity(text.len());
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else if c == '\n' || c == '\r' {
            clean.push(' ');
        } else {
            clean.push(c);
        }
    }
    clean
}

fn server_row(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    selected: bool,
    action: MenuAction,
) -> Result<(), UiPresentationError> {
    let mut state = canvas.interaction(view, Some(action));
    state.pressed &= state.hovered;
    let motion = canvas.feedback(state, true, selected, super::motion::Kind::Surface);
    super::sidebar::transparent_background(canvas, b, motion)?;
    if motion.focus > 0.0 {
        canvas.frame(
            b,
            super::theme::EDGE,
            super::motion::opacity(super::theme::OUTLINE, motion.focus),
        )?;
    }
    canvas.hit(action, b)?;
    Ok(())
}

fn server_text(
    canvas: &mut Canvas<'_>,
    b: Bounds,
    title: &str,
    caption: &str,
) -> Result<(), UiPresentationError> {
    let height = canvas.r(BODY.line
        + if caption.is_empty() {
            0.0
        } else {
            CAPTION.line
        });
    let y = (b[1] + b[3] - height) * 0.5;
    canvas.text_line(title, [b[0], y], b[2] - b[0], BODY, TEXT)?;
    if !caption.is_empty() {
        canvas.text_line(
            caption,
            [b[0], y + canvas.r(BODY.line)],
            b[2] - b[0],
            CAPTION,
            TEXT_DIMMER,
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Selection {
    Featured(usize),
    Saved(usize),
}

/// Hidden sections cannot supply details; an unavailable pick falls back in visible order.
fn selection(view: &MenuView) -> Option<Selection> {
    let prefs = view.settings_options.server_list();
    if let Some(index) = view
        .feeds
        .selected_saved
        .filter(|i| *i < view.servers.len())
        && prefs.visible(ServerGroup::Saved)
    {
        return Some(Selection::Saved(index));
    }
    if let Some(index) = view
        .feeds
        .selected_featured
        .filter(|i| *i < view.featured.len())
        && prefs.visible(experience_group(view, &view.featured[index]))
    {
        return Some(Selection::Featured(index));
    }
    prefs
        .order()
        .into_iter()
        .filter(|group| prefs.visible(*group))
        .find_map(|group| {
            if group == ServerGroup::Saved {
                (!view.servers.is_empty()).then_some(Selection::Saved(0))
            } else {
                view.featured
                    .iter()
                    .position(|server| experience_group(view, server) == group)
                    .map(Selection::Featured)
            }
        })
}

/// Vanilla's ping tiers: under 80 ms good, under 160 medium, else high; no
/// pong is offline.
fn ping_label(ping: Option<&PingInfo>, exact: bool) -> std::borrow::Cow<'static, str> {
    if let Some(ping) = ping.filter(|p| p.online && exact) {
        return format!("{} ms", ping.ping_ms).into();
    }
    match ping {
        None => "Loading ping",
        Some(ping) if !ping.online => "Offline",
        Some(ping) if ping.ping_ms < 80 => "Low ping",
        Some(ping) if ping.ping_ms < 160 => "Medium ping",
        Some(_) => "High ping",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_experience_shows_until_a_server_is_picked() {
        let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
        view.servers = vec![crate::menu::SavedServer {
            name: "Home".to_owned(),
            address: "127.0.0.1:19132".to_owned(),
            favorite: false,
            last_joined_unix: 0,
        }];
        assert_eq!(selection(&view), Some(Selection::Saved(0)));
        view.featured = ["First", "Second"]
            .into_iter()
            .map(|name| MenuServerCard {
                name: name.into(),
                address: name.into(),
                caption: String::new(),
                image_path: String::new(),
                icon: None,
            })
            .collect();
        assert_eq!(selection(&view), Some(Selection::Featured(0)));
        view.feeds.select_saved(0);
        assert_eq!(selection(&view), Some(Selection::Saved(0)));
        view.feeds.select(1);
        assert_eq!(selection(&view), Some(Selection::Featured(1)));
    }

    #[test]
    fn ping_labels_follow_the_round_trip() {
        let pong = |ping_ms| PingInfo {
            motd: String::new(),
            online: true,
            players: 1,
            max_players: 2,
            ping_ms,
        };
        assert_eq!(ping_label(None, false), "Loading ping");
        assert_eq!(ping_label(Some(&PingInfo::default()), false), "Offline");
        assert_eq!(ping_label(Some(&pong(20)), false), "Low ping");
        assert_eq!(ping_label(Some(&pong(120)), false), "Medium ping");
        assert_eq!(ping_label(Some(&pong(500)), false), "High ping");
        assert_eq!(ping_label(Some(&pong(20)), true), "20 ms");
        assert_eq!(ping_label(Some(&pong(120)), true), "120 ms");
        assert_eq!(ping_label(Some(&pong(500)), true), "500 ms");
        assert_eq!(ping_label(None, true), "Loading ping");
        assert_eq!(ping_label(Some(&PingInfo::default()), true), "Offline");
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_activity_extent_includes_wrapped_text() {
        use crate::menu::{MenuGameCard, ServerDetails};
        let mut view = crate::menu::MenuView::new(true, "Test".into());
        let server = MenuServerCard {
            name: "Server".into(),
            address: "example.test".into(),
            caption: String::new(),
            image_path: String::new(),
            icon: None,
        };
        view.feeds.details.insert(
            server.address.clone(),
            ServerDetails {
                games: vec![
                    MenuGameCard {
                        description: "Long activity description ".repeat(100),
                        ..Default::default()
                    };
                    2
                ],
                ..Default::default()
            },
        );
        let font = crate::ui_runtime::presentation::tests::fixture_font();
        let (mut nodes, mut next, mut layouts) =
            (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
        let metrics = crate::ui_runtime::presentation::TextMetrics::for_viewport(
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
            Some(2),
        );
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        let bottom = details::details_content(
            &mut canvas,
            &view,
            &server,
            0,
            [0.0, 0.0, 500.0],
            &HashMap::new(),
        )
        .unwrap();
        let text_bottom = canvas
            .nodes
            .iter()
            .filter(|node| matches!(node.visual(), ui::UiVisual::Text { .. }))
            .map(|node| node.bounds().max().y())
            .reduce(f32::max)
            .unwrap();
        assert!(
            bottom >= text_bottom,
            "content bottom {bottom}, text bottom {text_bottom}"
        );
    }
}
