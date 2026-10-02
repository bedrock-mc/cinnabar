//! Local-only renders of the launcher's play flow against fixture service data
//! (Realms, friends, featured servers, gatherings, pings, saved servers).
//! Skips without the gitignored carrier; PNGs go to `CINNABAR_FORM_SNAPSHOT_DIR`.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use ui::DpiScale;

use super::pack_harness::engine_presentation;
use crate::menu::MenuAction;
use crate::menu::{
    LiveEventCard, LocalWorldCard, MenuFriendCard, MenuGameCard, MenuRealmCard, MenuRuntime,
    MenuScreen, MenuServerCard, MenuView, PingInfo, SavedServer, ServerDetails, auth::AuthState,
};
use crate::ui_runtime::UiRuntime;

/// A solid-coloured PNG with a lighter band, written once per run.
fn art(dir: &std::path::Path, name: &str, size: [u32; 2], color: [u8; 3]) -> String {
    let path = dir.join(format!("{name}.png"));
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

fn server(name: &str, address: &str, caption: &str, image_path: String) -> MenuServerCard {
    MenuServerCard {
        name: name.to_owned(),
        address: address.to_owned(),
        caption: caption.to_owned(),
        image_path,
        icon: None,
    }
}

fn realm(name: &str, state: &str, member: bool, days_left: i32, expired: bool) -> MenuRealmCard {
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

/// A signed-in view carrying every service feed the play flow shows.
pub(super) fn fixture_view(dir: &std::path::Path) -> MenuView {
    let mut view = MenuRuntime::new(true, 2, "Steve".to_owned()).view();
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
    view.gatherings = vec![server(
        "Minecraft Live",
        "live.example.net:19132",
        "Live event",
        art(dir, "live", [256, 256], [140, 60, 200]),
    )];
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
            description: "Minigames with friends, every day.".to_owned(),
            news_title: "Season 5".to_owned(),
            news: "A new season of Treasure Wars is live.".to_owned(),
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

/// Captures a menu screen after its retained layout has settled.
pub(super) fn snapshot(view: &MenuView, name: &str) {
    snapshot_at(view, name, 0);
}

fn snapshot_at(view: &MenuView, name: &str, now_millis: u64) {
    snapshot_after(view, name, now_millis, 2);
}

/// `warm` frames first: a screen laid out off-thread needs a few to settle.
fn snapshot_after(view: &MenuView, name: &str, now_millis: u64, warm: usize) {
    snapshot_with_catalog(view, name, now_millis, warm, None);
}

/// Captures the installed pack before Cinnabar's settings layout overrides.
pub(super) fn snapshot_vanilla(view: &MenuView, name: &str) {
    let Some(carrier) = super::pack_harness::carrier() else {
        return;
    };
    let files = carrier.ui_files();
    let catalog =
        json_ui::Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
    snapshot_with_catalog(view, name, 0, 2, Some(Arc::new(catalog)));
}

/// Uses the usual offline rasterizer with an optional reference catalog.
fn snapshot_with_catalog(
    view: &MenuView,
    name: &str,
    now_millis: u64,
    warm: usize,
    catalog: Option<Arc<json_ui::Catalog>>,
) {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    if let Some(catalog) = catalog {
        presentation
            .form_presentation
            .engine
            .as_mut()
            .unwrap()
            .install_pack_catalog(catalog);
    }
    let mut runtime = UiRuntime::new(1);
    let lang = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.local/assets/compiled/vanilla-v1.mcbelang");
    if let Some(lang) = std::fs::read(lang)
        .ok()
        .and_then(|bytes| assets::RuntimeLangCatalog::decode(&bytes).ok())
    {
        runtime.set_lang_catalog(Arc::new(lang));
    }
    presentation.sync_menu_artwork(super::super::menu_artwork::view_paths(view));
    presentation.finish_menu_artwork();
    let dpi = DpiScale::new(2.0).unwrap();
    for _ in 0..warm {
        presentation.set_menu_view(Some(view.clone()));
        presentation
            .build(&runtime, now_millis, [2560, 1440], dpi)
            .unwrap();
        if warm > 2 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    presentation.set_menu_view(Some(view.clone()));
    let input = presentation
        .build(&runtime, now_millis, [2560, 1440], dpi)
        .unwrap();
    super::snapshot::write(&input, name);
}

// Writes PNGs of each play-flow state (local only).
#[test]
fn snapshot_play_flow() {
    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let base = fixture_view(&dir);
    let at = |screen: MenuScreen| {
        let mut view = base.clone();
        view.screen = screen;
        view
    };
    snapshot(&at(MenuScreen::Home), "flow-home");
    snapshot(&at(MenuScreen::Play), "flow-play-worlds");
    snapshot(&at(MenuScreen::Social), "flow-play-realms");
    let mut closed = at(MenuScreen::Social);
    closed.feeds.selected_realm = Some(1);
    snapshot(&closed, "flow-play-realms-closed");
    let mut servers = at(MenuScreen::Servers);
    snapshot(&servers, "flow-play-servers");
    servers.feeds.selected_featured = Some(0);
    snapshot(&servers, "flow-play-servers-featured");
    servers.feeds.select_saved(0);
    snapshot(&servers, "flow-play-servers-saved");
    servers.dialog = Some(crate::menu::MenuDialog::RemoveSaved(0));
    snapshot(&servers, "flow-remove-server");
    snapshot(&at(MenuScreen::Friends), "flow-friends");
    let mut add = at(MenuScreen::AddServer);
    add.name = "Home server".to_owned();
    add.address = "192.168.1.20:19132".to_owned();
    add.editing = Some(0);
    snapshot(&add, "flow-edit-server");
}

// Writes PNGs of the settings screen, the signing-in start screen and two
// frames of the connecting screen's loading bar (local only).
#[test]
fn snapshot_settings_signing_in_and_progress() {
    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let base = fixture_view(&dir);
    let mut settings = base.clone();
    settings.screen = MenuScreen::Settings;
    snapshot(&settings, "flow-settings");
    let mut signing_in = base.clone();
    signing_in.screen = MenuScreen::Home;
    signing_in.auth_state = AuthState::Checking;
    snapshot(&signing_in, "flow-home-signing-in");
    let mut connecting = base;
    connecting.connecting = true;
    connecting.message = Some("Connecting...".to_owned());
    snapshot_at(&connecting, "flow-connecting-0", 1_000);
    snapshot_at(&connecting, "flow-connecting-1", 1_350);
}

// The connecting screen's loading bar is a flip-book: later frames paint other
// texels over the same cached layout.
#[test]
fn the_loading_bar_animates_over_its_cached_layout() {
    let Some(mut presentation) = engine_presentation() else {
        return;
    };
    let bar = "textures/ui/loading_bar";
    let bar_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.local")
        .join(crate::install_layout::vanilla_pack_relative())
        .join(format!("{bar}.png"));
    assert!(
        bar_file.is_file(),
        "the animation fixture requires the pinned vanilla loading bar at {}",
        bar_file.display()
    );
    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let mut view = fixture_view(&dir);
    view.connecting = true;
    view.message = Some("Connecting...".to_owned());
    let runtime = UiRuntime::new(1);
    let dpi = DpiScale::new(1.0).unwrap();
    let frame = |presentation: &mut super::super::UiPresentationRuntime, now_millis| {
        presentation.set_menu_view(Some(view.clone()));
        presentation
            .build(&runtime, now_millis, [1280, 720], dpi)
            .unwrap()
    };
    // On-demand texture decodes may finish on workers after the inline budget.
    // Hold the animation clock still until the strip itself can be drawn.
    let started = std::time::Instant::now();
    loop {
        frame(&mut presentation, 1_000);
        let resident = presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .textures
            .lock()
            .placement(bar)
            .is_some();
        if resident {
            break;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the loading bar decode did not become resident: {}",
            bar_file.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let first = frame(&mut presentation, 1_000);
    let later = frame(&mut presentation, 1_350);
    let positions = |input: &render::UiRenderInput| {
        input
            .vertices
            .iter()
            .map(|vertex| vertex.position)
            .collect::<Vec<_>>()
    };
    let uvs = |input: &render::UiRenderInput| {
        input
            .vertices
            .iter()
            .map(|vertex| vertex.uv)
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(&first), positions(&later), "one layout");
    assert_ne!(uvs(&first), uvs(&later), "another frame of the strip");
}

// The disconnect screen words the failure as vanilla does and offers OK, which
// leaves it for the menu.
#[test]
fn the_disconnect_screen_has_a_way_back() {
    let Some(mut presentation) = engine_presentation() else {
        return;
    };
    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let mut view = fixture_view(&dir);
    view.screen = MenuScreen::Play;
    view.disconnect_message =
        Some("network session failed: Bedrock session failed: Connection closed".to_owned());
    snapshot(&view, "flow-disconnect");
    presentation.set_menu_view(Some(view));
    let runtime = UiRuntime::new(1);
    let dpi = DpiScale::new(1.0).unwrap();
    let metrics = super::super::TextMetrics::for_viewport([1280, 720], dpi, None);
    let mut nodes = Vec::new();
    let mut next = 1;
    let hits = presentation
        .append_menu(&runtime, &mut nodes, &mut next, metrics, 1280.0, 720.0)
        .unwrap();
    assert!(
        hits.iter()
            .any(|(action, _)| *action == crate::menu::MenuAction::DismissDialog),
        "{hits:?}"
    );
    let texts = super::pack_harness::drawn_texts(&nodes);
    assert!(
        !texts.iter().any(|text| text.contains("session failed")),
        "the raw chain stays in the log: {texts:?}"
    );
}

// An overflowing server list scrolls under the wheel, bringing hidden rows into reach.
#[test]
fn the_server_list_scrolls_under_the_wheel() {
    let Some(mut presentation) = engine_presentation() else {
        return;
    };
    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let mut view = fixture_view(&dir);
    view.screen = MenuScreen::Servers;
    view.featured = (0..20)
        .map(|index| {
            let address = format!("s{index}.example.net:19132");
            server(
                &format!("Server {index}"),
                &address,
                "Minigames",
                String::new(),
            )
        })
        .collect();
    let runtime = UiRuntime::new(1);
    let dpi = DpiScale::new(1.0).unwrap();
    let frame = |presentation: &mut super::super::UiPresentationRuntime| {
        presentation.set_menu_view(Some(view.clone()));
        let input = presentation.build(&runtime, 0, [1280, 720], dpi).unwrap();
        (presentation.menu_hit_targets.clone(), input)
    };
    let row = |hits: &[(MenuAction, ui::UiRect)], index| {
        hits.iter()
            .find(|(action, _)| *action == MenuAction::SelectFeatured(index))
            .map(|(_, bounds)| *bounds)
    };
    let (before, input) = frame(&mut presentation);
    super::snapshot::write(&input, "flow-servers-top");
    assert!(
        row(&before, 19).is_none(),
        "the last row starts below the fold"
    );
    let first = row(&before, 0).unwrap().min();
    let point = ui::UiPoint::new(first.x() + 4.0, first.y() + 4.0).unwrap();
    assert!(presentation.scroll_menu(point, -100.0, false));
    let (after, input) = frame(&mut presentation);
    super::snapshot::write(&input, "flow-servers-scrolled");
    assert!(row(&after, 19).is_some(), "the last row scrolled into view");
    assert!(row(&after, 0).is_none(), "the first row scrolled out");
}

// The settings screen's JSON-UI scroll views take the wheel like the OreUI lists.
#[test]
fn the_settings_panes_take_the_wheel() {
    let Some(mut presentation) = engine_presentation() else {
        return;
    };
    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let mut view = fixture_view(&dir);
    view.screen = MenuScreen::Settings;
    let runtime = UiRuntime::new(1);
    let dpi = DpiScale::new(1.0).unwrap();
    presentation.set_menu_view(Some(view.clone()));
    presentation.build(&runtime, 0, [1280, 720], dpi).unwrap();
    let left = ui::UiPoint::new(200.0, 400.0).unwrap();
    assert!(presentation.scroll_menu(left, -40.0, false));
    presentation.set_menu_view(Some(view));
    let input = presentation.build(&runtime, 0, [1280, 720], dpi).unwrap();
    super::snapshot::write(&input, "flow-settings-scrolled");
}

// Writes PNGs of the local-world screens driven through the module (local only).
#[test]
fn snapshot_local_worlds() {
    use crate::local_worlds::{Event, Input, PromptButton, Tab, WorldsMenu};
    use protocol::world_control::{
        Backend, Difficulty, GameMode, Generator, Prefs, Setup, SetupState, UnavailableReason,
        World, WorldState, WorldStatus,
    };
    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let mut base = fixture_view(&dir);
    base.screen = MenuScreen::Play;
    let world = World {
        id: "0123456789abcdef".to_owned(),
        name: "My World".to_owned(),
        game_mode: GameMode::Survival,
        generator: Generator::Normal,
        difficulty: Difficulty::Normal,
        backend: Backend::Bds,
        seed: 1,
        created_unix: 1_790_553_600,
        last_played_unix: 1_790_553_600,
        size_bytes: 12 * 1024 * 1024,
    };
    let shot = |menu: &WorldsMenu, name: &str| {
        let mut view = base.clone();
        view.local = menu.view();
        snapshot(&view, name);
    };
    let mut menu = WorldsMenu::default();
    menu.update(Input::Refresh);
    menu.apply(Event::Listed(vec![world]));
    menu.update(Input::BeginCreate);
    shot(&menu, "local-create-general");
    menu.update(Input::SelectTab(Tab::Advanced));
    shot(&menu, "local-create-advanced");
    menu.update(Input::Back);
    menu.update(Input::OpenTemplates);
    shot(&menu, "local-templates");
    menu.update(Input::Back);
    menu.update(Input::BeginEdit(0));
    shot(&menu, "local-edit");
    menu.update(Input::RequestDelete);
    shot(&menu, "local-delete-confirm");
    menu.update(Input::Back);
    menu.update(Input::Back);
    let status = |state: WorldState, setup: Option<Setup>, reason| WorldStatus {
        state,
        world_id: Some("0123456789abcdef".to_owned()),
        backend: Some(Backend::Bds),
        paused: false,
        pause_supported: false,
        error: None,
        setup,
        backend_unavailable_reason: reason,
    };
    menu.update(Input::Play);
    let mut download = Setup {
        state: SetupState::Downloading,
        version: Some("1.26.52.3".to_owned()),
        bytes_done: 40 * 1024 * 1024,
        bytes_total: 90 * 1024 * 1024,
        layers_done: 0,
        layers_total: 0,
        eula_accepted: true,
        error: None,
        runtime: "container".to_owned(),
        reason: None,
    };
    menu.apply(Event::Status(status(
        WorldState::Starting,
        Some(download.clone()),
        None,
    )));
    let mut progress = base.clone();
    progress.local = menu.view();
    snapshot_after(&progress, "local-progress-download", 0, 40);
    menu.update(Input::Back);
    download.state = SetupState::Unsupported;
    menu.apply(Event::Prefs(
        Prefs::default(),
        status(
            WorldState::Idle,
            Some(download),
            Some(UnavailableReason::DockerMissing),
        ),
    ));
    menu.update(Input::BeginCreate);
    shot(&menu, "local-docker-missing");
    menu.update(Input::Prompt(PromptButton::CreateFlat));
    shot(&menu, "local-create-flat-only");
}
