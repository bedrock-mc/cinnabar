//! Service projections and original artwork for offline launcher frames.
use crate::menu::{
    LiveEventCard, LocalWorldCard, MenuFriendCard, MenuGameCard, MenuRealmCard, MenuServerCard,
    MenuView, PingInfo, SavedServer, ServerDetails, auth::AuthState,
};
use std::collections::HashMap;

/// Builds deterministic offline launcher fixture data.
pub(crate) fn art(dir: &std::path::Path, name: &str, size: [u32; 2], color: [u8; 3]) -> String {
    static NEXT_ART: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let index = NEXT_ART.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("{name}-{}-{index}.png", std::process::id()));
    let image = image::RgbaImage::from_fn(size[0], size[1], |_, y| {
        let lift = if y < size[1] / 3 { 40 } else { 0 };
        image::Rgba([
            color[0].saturating_add(lift),
            color[1].saturating_add(lift),
            color[2].saturating_add(lift),
            255,
        ])
    });
    image.save(&path).unwrap();
    path.to_string_lossy().into_owned()
}

/// Builds deterministic offline launcher fixture data.
pub(crate) fn server(
    name: &str,
    address: &str,
    caption: &str,
    image_path: String,
) -> MenuServerCard {
    MenuServerCard {
        name: name.to_owned(),
        address: address.to_owned(),
        caption: caption.to_owned(),
        image_path,
        icon: None,
    }
}

/// Builds deterministic offline launcher fixture data.
pub(crate) fn realm(
    name: &str,
    state: &str,
    member: bool,
    days_left: i32,
    expired: bool,
) -> MenuRealmCard {
    MenuRealmCard {
        name: name.to_owned(),
        state: state.to_owned(),
        target: format!("realm/{name}"),
        address: String::new(),
        owner: if member {
            "Alex".to_owned()
        } else {
            String::new()
        },
        online_players: 3,
        max_players: 10,
        days_left,
        expired,
        member,
    }
}

/// Builds deterministic offline launcher fixture data.
pub fn fixture_view(dir: &std::path::Path) -> MenuView {
    let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
    view.auth_state = AuthState::Authenticated;
    view.catalog_loading = false;
    view.featured = vec![
        server(
            "The Hive",
            "geo.hivebedrock.network:19132",
            "Minigames",
            art(dir, "hive", [256, 256], [220, 160, 20]),
        ),
        server(
            "CubeCraft",
            "play.cubecraft.net:19132",
            "Skywars, EggWars",
            art(dir, "cubecraft", [256, 256], [30, 110, 200]),
        ),
    ];
    view.realms = vec![
        realm("Steve's Realm", "open", false, 21, false),
        realm("Build Club", "closed", false, 3, false),
        realm("Alex's Realm", "open", true, 0, false),
        realm(
            "ADD ONYXJAVA AS A FRIEND TO JOIN ONYX!",
            "open",
            false,
            10,
            false,
        ),
    ];
    view.friends = vec![
        MenuFriendCard {
            gamertag: "Alex".to_owned(),
            world_name: "Sky Base".to_owned(),
            members: "2/8 players".to_owned(),
            xuid: "2535400000000001".to_owned(),
        },
        MenuFriendCard {
            gamertag: "Notch".to_owned(),
            world_name: "Survival 2".to_owned(),
            members: "1/8 players".to_owned(),
            xuid: "2535400000000002".to_owned(),
        },
    ];
    view.local_worlds = vec![LocalWorldCard {
        name: "My World".to_owned(),
        game_mode: "Survival".to_owned(),
        world_type: crate::local_worlds::NORMAL_WORLD_LABEL.to_owned(),
        date: "9/30/2026".to_owned(),
        size: "12 MB".to_owned(),
    }];
    view.servers = vec![
        SavedServer {
            name: "Home server".to_owned(),
            address: "192.168.1.20:19132".to_owned(),
            favorite: false,
            last_joined_unix: 0,
        },
        SavedServer {
            name: "Test".to_owned(),
            address: "test.example.net:19132".to_owned(),
            favorite: true,
            last_joined_unix: 0,
        },
    ];
    let pong = |players, max_players, ping_ms| PingInfo {
        motd: String::new(),
        online: true,
        players,
        max_players,
        ping_ms,
    };
    view.feeds.pings = HashMap::from([
        (
            "geo.hivebedrock.network:19132".to_owned(),
            pong(21_345, 100_000, 40),
        ),
        (
            "play.cubecraft.net:19132".to_owned(),
            pong(8_210, 50_000, 180),
        ),
        ("192.168.1.20:19132".to_owned(), pong(2, 10, 3)),
        ("test.example.net:19132".to_owned(), PingInfo::default()),
    ]);
    view.feeds.details.insert(
        "geo.hivebedrock.network:19132".to_owned(),
        ServerDetails {
            group: String::new(),
            player_count: None,
            description: "Minigames with friends, every day.".to_owned(),
            news_title: "Season 5".to_owned(),
            news: "A new season of Treasure Wars is live.".to_owned(),
            banner: String::new(),
            logo_url: String::new(),
            screenshots: vec![art(dir, "hive_banner", [512, 154], [180, 120, 30])],
            games: vec![MenuGameCard {
                title: "Treasure Wars".to_owned(),
                subtitle: "Teams of four".to_owned(),
                description: "Protect your treasure.".to_owned(),
                image_path: art(dir, "treasure", [128, 128], [200, 60, 60]),
            }],
        },
    );
    view.feeds.profile.gamertag = "Steve".to_owned();
    view.feeds.home.realm_invites = 2;
    view.feeds.home.inbox_unread = 1;
    view.feeds.home.live_event = Some(LiveEventCard {
        button_text: "Learn More".to_owned(),
        caption: "Minecraft Live".to_owned(),
        countdown: false,
        start_unix: 0,
        badge_path: art(dir, "badge", [256, 128], [40, 90, 200]),
        address: "live.example.net:19132".to_owned(),
        route_to_servers: false,
    });
    view
}
