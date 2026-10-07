//! The vanilla play screen's bindings: Worlds (local worlds and Realms), Friends
//! (joinable friend worlds and member Realms) and Servers (saved servers, and
//! the featured list of servers with the selected one's info panel), plus how
//! its world and server presses map back to menu actions.

use json_ui::{CollectionItem, DataSource, HitRegion, Scalar};

use crate::menu::{
    MenuAction, MenuRealmCard, MenuScreen, MenuServerCard, MenuView, PingInfo, pingable,
};

const FEATURED: &str = "third_party_server_network_worlds";
const PERSONAL_REALMS: &str = "personal_realms";
const FRIEND_REALMS: &str = "friends_realms";
/// Round trips (ms) below which the ping icon is green, then yellow; needs native measurement.
const PING_GREEN_BELOW: u32 = 150;
const PING_YELLOW_BELOW: u32 = 300;

/// The ping icon for a pong: offline red until answered, then by round trip.
fn ping_texture(ping: Option<&PingInfo>) -> &'static str {
    match ping {
        Some(ping) if ping.online && ping.ping_ms < PING_GREEN_BELOW => "textures/ui/Ping_Green",
        Some(ping) if ping.online && ping.ping_ms < PING_YELLOW_BELOW => "textures/ui/Ping_Yellow",
        Some(ping) if ping.online => "textures/ui/Ping_Red",
        _ => "textures/ui/Ping_Offline_Red",
    }
}

/// A featured row's ping icon; an experience, having no server to ping, shows none.
fn featured_ping_texture(pingable: bool, ping: Option<&PingInfo>) -> &'static str {
    if pingable { ping_texture(ping) } else { "" }
}

/// Server pongs include capacity; missing or offline pongs leave the label empty.
fn player_count(ping: Option<&PingInfo>) -> String {
    match ping {
        Some(ping) if ping.online => format!("{}/{}", ping.players, ping.max_players),
        _ => String::new(),
    }
}

/// Experience details show positive service counts as plain decimal numbers, without capacity.
fn featured_player_count(view: &MenuView, address: &str) -> String {
    if pingable(address) {
        return player_count(view.feeds.pings.get(address));
    }
    view.feeds
        .details
        .get(address)
        .and_then(|details| details.player_count)
        .filter(|count| *count > 0)
        .map_or_else(String::new, |count| count.to_string())
}

fn text(value: impl Into<String>) -> Scalar {
    Scalar::Text(value.into())
}

fn flag(data: &mut DataSource, name: &str, on: bool) {
    data.set_global(name, Scalar::Bool(on));
}

/// The featured servers, as the Servers tab lists them.
fn featured(view: &MenuView) -> impl Iterator<Item = &MenuServerCard> {
    view.featured.iter()
}

pub(super) fn bind(view: &MenuView, data: &mut DataSource) {
    let tab = match view.screen {
        MenuScreen::Social => 1,
        MenuScreen::Servers => 2,
        _ => 0,
    };
    data.select_radio("navigation_tab", tab);
    for name in [
        "#is_network_available_and_multiplayer_visible",
        "#friends_grid_visible",
        "#servers_grid_visible",
        "#featured_servers_visible",
        "#featured_servers_visible_and_available",
        "#realms_grids_visible",
        "#local_worlds_visible",
        "#is_additional_server_label_visible",
    ] {
        flag(data, name, true);
    }
    local_worlds(view, data);
    network_worlds(view, data);
    featured_servers(view, data);
    realms(view, data);
}

fn local_worlds(view: &MenuView, data: &mut DataSource) {
    let worlds = view
        .local_worlds
        .iter()
        .map(|world| {
            CollectionItem::default()
                .with("#local_world_name", text(world.name.clone()))
                .with("#local_world_game_mode", text(world.game_mode.clone()))
                .with("#local_world_date", text(world.date.clone()))
                .with("#local_worldfile_size", text(world.size.clone()))
        })
        .collect::<Vec<_>>();
    data.set_global("#world_item_count", text(worlds.len().to_string()));
    data.set_collection("local_worlds", worlds);
}

fn network_worlds(view: &MenuView, data: &mut DataSource) {
    let network = |header: &str, details: &str, players: &str| {
        CollectionItem::default()
            .with("#network_world_header", text(header))
            .with("#network_world_details", text(details))
            .with("#network_world_player_count", text(players))
            .with("#network_world_button_enabled", Scalar::Bool(true))
            .with("#game_online", Scalar::Bool(true))
    };
    let friends = view
        .friends
        .iter()
        .map(|friend| network(&friend.world_name, &friend.gamertag, &friend.members))
        .collect::<Vec<_>>();
    flag(data, "#no_friends_grid_message_visible", friends.is_empty());
    data.set_global("#friend_world_item_count", text(friends.len().to_string()));
    data.set_collection("friends_network_worlds", friends);
    let offset = featured(view).count();
    let saved = view
        .servers
        .iter()
        .enumerate()
        .map(|(index, server)| {
            let ping = view.feeds.pings.get(&server.address);
            network(&server.name, &server.address, &player_count(ping))
                .with("#texture_name", text(ping_texture(ping)))
                .with(
                    "#additional_server_toggle_index",
                    Scalar::Num((offset + index) as f64),
                )
        })
        .collect::<Vec<_>>();
    data.set_global("#server_world_item_count", text(saved.len().to_string()));
    data.set_collection("servers_network_worlds", saved);
}

fn featured_servers(view: &MenuView, data: &mut DataSource) {
    let items = featured(view)
        .enumerate()
        .map(|(index, server)| {
            let ping = view.feeds.pings.get(&server.address);
            let pingable = pingable(&server.address);
            CollectionItem::default()
                .with("#third_party_toggle_index", Scalar::Num(index as f64))
                .with("#server_player_count", text(player_count(ping)))
                .with("#texture_name", text(featured_ping_texture(pingable, ping)))
                .with(
                    "#is_network_available_and_ping_not_loading",
                    Scalar::Bool(ping.is_some() || !pingable),
                )
                .with("#third_party_server_name", text(server.name.clone()))
                .with("#third_party_server_message", text(server.caption.clone()))
                .with(
                    "#third_party_server_logo_texture_path",
                    text(server.image_path.clone()),
                )
                .with("#is_server_info_available_collection", Scalar::Bool(true))
        })
        .collect();
    data.set_collection(FEATURED, items);
    let selected = view
        .feeds
        .selected_featured
        .and_then(|index| featured(view).nth(index));
    flag(data, "#is_server_info_available", selected.is_some());
    let Some(server) = selected else {
        return;
    };
    if let Some(index) = view.feeds.selected_featured {
        data.select_radio("server_navigation_toggle", index);
    }
    let ping = view.feeds.pings.get(&server.address);
    let pingable = pingable(&server.address);
    flag(data, "#ping_ready_thirdparty", ping.is_some() || !pingable);
    data.set_global(
        "#info_third_party_server_player_count",
        text(featured_player_count(view, &server.address)),
    );
    data.set_global(
        "#info_ping_texture_name",
        text(featured_ping_texture(pingable, ping)),
    );
    data.set_global(
        "#info_server_ping",
        text(
            ping.filter(|ping| ping.online)
                .map_or_else(String::new, |ping| format!("{} ms", ping.ping_ms)),
        ),
    );
    data.set_global("#info_third_party_server_name", text(server.name.clone()));
    data.set_global(
        "#info_third_party_server_logo_texture_path",
        text(server.image_path.clone()),
    );
    flag(
        data,
        "#info_third_party_screenshot_visible",
        !server.image_path.is_empty(),
    );
    let Some(details) = view.feeds.details.get(&server.address) else {
        return;
    };
    flag(
        data,
        "#server_has_description",
        !details.description.is_empty(),
    );
    data.set_global("#description_label", text(details.description.clone()));
    // Long text opens collapsed behind its "read more" toggle.
    let expanded = view.feeds.description_expanded;
    flag(data, "#description_is_read_more", !expanded);
    flag(data, "#description_is_read_less", expanded);
    flag(data, "#server_has_news", !details.news.is_empty());
    data.set_global("#news_text", text(details.news.clone()));
    data.set_global("#news_label", text(details.news_title.clone()));
    let expanded = view.feeds.news_expanded;
    flag(data, "#news_is_read_more", !expanded);
    flag(data, "#news_is_read_less", expanded);
    let screenshots = details
        .screenshots
        .iter()
        .enumerate()
        .map(|(index, path)| {
            CollectionItem::default()
                .with("#screenshot_texture", text(path.clone()))
                .with("#this_screenshot_selected", Scalar::Bool(index == 0))
        })
        .collect::<Vec<_>>();
    flag(data, "#server_has_screenshots", !screenshots.is_empty());
    data.set_global(
        "#screenshot_collection_length",
        Scalar::Num(screenshots.len() as f64),
    );
    data.set_collection("server_screenshot_collection", screenshots);
    let games = details
        .games
        .iter()
        .map(|game| {
            CollectionItem::default()
                .with("#available_game_title", text(game.title.clone()))
                .with("#available_game_subtitle", text(game.subtitle.clone()))
                .with(
                    "#available_game_description",
                    text(game.description.clone()),
                )
                .with("#available_game_image", text(game.image_path.clone()))
                .with(
                    "#available_game_image_visible",
                    Scalar::Bool(!game.image_path.is_empty()),
                )
        })
        .collect::<Vec<_>>();
    flag(data, "#server_has_games", !games.is_empty());
    data.set_global("#games_collection_length", Scalar::Num(games.len() as f64));
    data.set_collection("server_games_collection", games);
}

fn realms(view: &MenuView, data: &mut DataSource) {
    let item = |realm: &MenuRealmCard| {
        let open = realm.state.eq_ignore_ascii_case("open") && !realm.expired;
        let players = if realm.max_players > 0 {
            format!("{}/{}", realm.online_players, realm.max_players)
        } else {
            realm.online_players.to_string()
        };
        let details = if realm.member {
            realm.owner.clone()
        } else {
            realm.state.clone()
        };
        CollectionItem::default()
            .with("#realms_world_header", text(realm.name.clone()))
            .with("#realms_world_details", text(details))
            .with("#realms_world_player_count", text(players))
            .with("#realms_game_online", Scalar::Bool(open))
            .with(
                "#realms_game_offline",
                Scalar::Bool(!open && !realm.expired),
            )
            .with("#realms_game_unavailable", Scalar::Bool(realm.expired))
            .with(
                "#realms_world_expiry_notification_visible",
                Scalar::Bool(!realm.member && (realm.expired || realm.days_left <= 7)),
            )
    };
    let personal = view
        .realms
        .iter()
        .filter(|realm| !realm.member)
        .map(item)
        .collect::<Vec<_>>();
    let friends = view
        .realms
        .iter()
        .filter(|realm| realm.member)
        .map(item)
        .collect::<Vec<_>>();
    flag(data, "#personal_realms_grid_visible", !personal.is_empty());
    flag(data, "#friends_realms_visible", !friends.is_empty());
    flag(data, "#joinable_realms_panel_visible", !friends.is_empty());
    data.set_collection(PERSONAL_REALMS, personal);
    data.set_collection(FRIEND_REALMS, friends);
}

/// Joining the featured-list entry at `index`.
pub(super) fn play_featured(view: &MenuView, index: usize) -> Option<MenuAction> {
    (index < view.featured.len()).then_some(MenuAction::PlayFeatured(index))
}

/// The action for a press on the Servers tab's featured list or info panel.
pub(super) fn featured_action(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    let index = match region.collection.as_deref() {
        Some(FEATURED) => region.collection_index?,
        _ => view.feeds.selected_featured?,
    };
    play_featured(view, index)
}

/// The Realms entry a grid press starts, as an index into all realms.
pub(super) fn realm_action(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    let index = region.collection_index?;
    let member = region.collection.as_deref() == Some(FRIEND_REALMS);
    view.realms
        .iter()
        .enumerate()
        .filter(|(_, realm)| realm.member == member)
        .nth(index)
        .map(|(overall, _)| MenuAction::PlayRealm(overall))
}

/// Selecting a featured-list entry shows it in the info panel.
pub(super) fn is_featured(region: &HitRegion) -> bool {
    region.collection.as_deref() == Some(FEATURED)
}

#[cfg(test)]
mod tests {
    use json_ui::{HitKind, RectOut};

    use super::*;

    /// Reads the selected server's count through the same global binding as the installed label.
    fn bound_featured_count(view: &MenuView) -> serde_json::Value {
        let name = "#info_third_party_server_player_count";
        let label = json_ui::ResolvedControl {
            name: "count".into(),
            control_type: Some("label".into()),
            base: None,
            unresolved_base: None,
            properties: [
                ("text".into(), serde_json::json!(name)),
                (
                    "bindings".into(),
                    serde_json::json!([{ "binding_name": name }]),
                ),
            ]
            .into(),
            children: Vec::new(),
            factory: None,
        };
        let mut data = DataSource::new();
        bind(view, &mut data);
        json_ui::bind(&label, &data, &json_ui::EmptyLibrary).properties["text"].clone()
    }

    #[test]
    fn selected_experience_player_count_reaches_existing_detail_binding() {
        use crate::menu::ServerDetails;

        let mut view = MenuView::new(true, "Steve".to_owned());
        let address = format!("{}example", crate::menu::EXPERIENCE_ADDRESS_PREFIX);
        view.featured.push(MenuServerCard {
            address: address.clone(),
            ..card("experience")
        });
        view.feeds.selected_featured = Some(0);
        for (count, expected) in [(Some(12_345), "12345"), (Some(0), ""), (None, "")] {
            view.feeds.details.insert(
                address.clone(),
                ServerDetails {
                    player_count: count,
                    ..Default::default()
                },
            );
            assert_eq!(bound_featured_count(&view), expected);
        }
    }

    /// An experience has no server to ping, so its row shows no ping icon instead of a stale one.
    #[test]
    fn experiences_show_no_ping_icon() {
        let pong = PingInfo {
            motd: String::new(),
            online: true,
            players: 1,
            max_players: 10,
            ping_ms: 20,
        };
        assert!(!pingable("gathering/5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f"));
        assert_eq!(featured_ping_texture(false, None), "");
        assert_eq!(
            featured_ping_texture(true, Some(&pong)),
            "textures/ui/Ping_Green"
        );
        assert_eq!(
            featured_ping_texture(true, None),
            "textures/ui/Ping_Offline_Red"
        );
    }

    #[test]
    fn experience_player_counts_use_details_and_hide_unavailable_values() {
        use crate::menu::ServerDetails;

        let mut view = MenuView::new(true, "Steve".to_owned());
        let address = format!("{}example", crate::menu::EXPERIENCE_ADDRESS_PREFIX);
        view.feeds.pings.insert(
            address.clone(),
            PingInfo {
                motd: String::new(),
                online: true,
                players: 2,
                max_players: 10,
                ping_ms: 20,
            },
        );
        assert_eq!(featured_player_count(&view, &address), "");
        for (count, expected) in [
            (None, ""),
            (Some(0), ""),
            (Some(-1), ""),
            (Some(1), "1"),
            (Some(12_345), "12345"),
            (Some(i64::MAX), "9223372036854775807"),
        ] {
            view.feeds.details.insert(
                address.clone(),
                ServerDetails {
                    player_count: count,
                    ..Default::default()
                },
            );
            assert_eq!(featured_player_count(&view, &address), expected);
        }
    }

    #[test]
    fn pingable_featured_servers_keep_pong_counts_and_capacity() {
        use crate::menu::ServerDetails;

        let mut view = MenuView::new(true, "Steve".to_owned());
        let address = "server.test:19132";
        view.feeds.details.insert(
            address.to_owned(),
            ServerDetails {
                player_count: Some(12_345),
                ..Default::default()
            },
        );
        assert_eq!(featured_player_count(&view, address), "");
        view.feeds.pings.insert(
            address.to_owned(),
            PingInfo {
                motd: String::new(),
                online: true,
                players: 3,
                max_players: 20,
                ping_ms: 40,
            },
        );
        assert_eq!(featured_player_count(&view, address), "3/20");
    }

    fn card(name: &str) -> MenuServerCard {
        MenuServerCard {
            name: name.to_owned(),
            address: format!("{name}.test:19132"),
            caption: String::new(),
            image_path: String::new(),
            icon: None,
        }
    }

    fn realm(name: &str, member: bool) -> MenuRealmCard {
        MenuRealmCard {
            name: name.to_owned(),
            state: "OPEN".to_owned(),
            target: String::new(),
            address: String::new(),
            owner: String::new(),
            online_players: 0,
            max_players: 10,
            days_left: 30,
            expired: false,
            member,
        }
    }

    fn press(collection: Option<&str>, index: Option<usize>) -> HitRegion {
        let rect = RectOut {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        };
        HitRegion {
            key: "/screen/item".to_owned(),
            name: "item".to_owned(),
            kind: HitKind::Button,
            rect,
            clip: rect,
            layer: 0,
            order: 0,
            pressed: None,
            control_name: None,
            collection_index: index,
            collection: collection.map(str::to_owned),
            enabled: true,
            checked: None,
            max_length: None,
            group_index: None,
            renderer: None,
            drag_axes: [false; 2],
            sound: None,
            input: Default::default(),
            focus: None,
            widget: Default::default(),
            collections: Vec::new(),
            modal_root: None,
        }
    }

    #[test]
    fn the_featured_list_joins_the_pressed_or_selected_server() {
        let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
        view.featured = vec![card("a"), card("g")];
        assert_eq!(play_featured(&view, 0), Some(MenuAction::PlayFeatured(0)));
        assert_eq!(play_featured(&view, 1), Some(MenuAction::PlayFeatured(1)));
        assert_eq!(play_featured(&view, 2), None);
        // The info panel's join button joins the selected entry.
        view.feeds.selected_featured = Some(1);
        assert_eq!(
            featured_action(&view, &press(None, None)),
            Some(MenuAction::PlayFeatured(1))
        );
    }

    #[test]
    fn pongs_pick_the_ping_icon_and_player_count() {
        let pong = |ping_ms| PingInfo {
            motd: String::new(),
            online: true,
            players: 3,
            max_players: 20,
            ping_ms,
        };
        assert_eq!(ping_texture(None), "textures/ui/Ping_Offline_Red");
        assert_eq!(ping_texture(Some(&pong(40))), "textures/ui/Ping_Green");
        assert_eq!(ping_texture(Some(&pong(200))), "textures/ui/Ping_Yellow");
        assert_eq!(ping_texture(Some(&pong(900))), "textures/ui/Ping_Red");
        assert_eq!(player_count(Some(&pong(40))), "3/20");
        assert_eq!(player_count(Some(&PingInfo::default())), "");
    }

    #[test]
    fn realm_grids_index_into_all_realms() {
        let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
        view.realms = vec![
            realm("mine", false),
            realm("theirs", true),
            realm("ours", false),
        ];
        assert_eq!(
            realm_action(&view, &press(Some(FRIEND_REALMS), Some(0))),
            Some(MenuAction::PlayRealm(1))
        );
        assert_eq!(
            realm_action(&view, &press(Some(PERSONAL_REALMS), Some(1))),
            Some(MenuAction::PlayRealm(2))
        );
    }
}
