//! Local-only renders of the launcher's play flow against fixture service data
//! (Realms, friends, featured servers, pings, saved servers).
//! Skips without the gitignored carrier; PNGs go to `CINNABAR_FORM_SNAPSHOT_DIR`.

use ui::{DpiScale, UiVisual};

use super::pack_harness::engine_presentation;
use crate::menu::MenuAction;
use crate::menu::{MenuScreen, auth::AuthState};
use crate::ui_runtime::UiRuntime;

/// Captures the shipped menu routes without account or server fixtures.
#[test]
fn snapshot_shipped_font_routes() {
    let player = player_state::PlayerState::new(1);
    for (screen, name) in [
        (MenuScreen::Home, "fonts-home"),
        (MenuScreen::Play, "fonts-play"),
        (MenuScreen::Settings, "fonts-settings"),
        (MenuScreen::Profile, "fonts-profile"),
        (MenuScreen::Pause, "fonts-pause"),
    ] {
        let mut view = crate::menu::MenuView::new(true, "Player".into());
        view.screen = screen;
        view.over_world = screen == MenuScreen::Pause;
        snapshot(&player, &view, name);
    }
}

// Writes PNGs of each play-flow state (local only).
#[test]
fn snapshot_play_flow() {
    let player_runtime = player_state::PlayerState::new(1);

    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let base = fixture_view(&dir);
    let at = |screen: MenuScreen| {
        let mut view = base.clone();
        view.screen = screen;
        view
    };
    snapshot(&player_runtime, &at(MenuScreen::Home), "flow-home");
    snapshot(&player_runtime, &at(MenuScreen::Play), "flow-play-worlds");
    snapshot(&player_runtime, &at(MenuScreen::Social), "flow-play-realms");
    let mut closed = at(MenuScreen::Social);
    closed.feeds.selected_realm = Some(1);
    snapshot(&player_runtime, &closed, "flow-play-realms-closed");
    let mut servers = at(MenuScreen::Servers);
    snapshot(&player_runtime, &servers, "flow-play-servers");
    servers.feeds.selected_featured = Some(0);
    snapshot(&player_runtime, &servers, "flow-play-servers-featured");
    servers.feeds.select_saved(0);
    snapshot(&player_runtime, &servers, "flow-play-servers-saved");
    servers.dialog = Some(crate::menu::MenuDialog::RemoveSaved(0));
    snapshot(&player_runtime, &servers, "flow-remove-server");
    snapshot(&player_runtime, &at(MenuScreen::Friends), "flow-friends");
    let mut add = at(MenuScreen::AddServer);
    add.name = "Home server".to_owned();
    add.address = "192.168.1.20:19132".to_owned();
    add.editing = Some(0);
    snapshot(&player_runtime, &add, "flow-edit-server");
}

// Writes PNGs of the settings screen, the signing-in start screen and two
// frames of the connecting screen's loading indicator (local only).
#[test]
fn snapshot_settings_signing_in_and_progress() {
    let player_runtime = player_state::PlayerState::new(1);

    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let base = fixture_view(&dir);
    let mut settings = base.clone();
    settings.screen = MenuScreen::Settings;
    snapshot(&player_runtime, &settings, "flow-settings");
    let mut signing_in = base.clone();
    signing_in.screen = MenuScreen::Home;
    signing_in.auth_state = AuthState::Checking;
    snapshot(&player_runtime, &signing_in, "flow-home-signing-in");
    let mut connecting = base;
    connecting.connecting = true;
    connecting.message = Some("Connecting...".to_owned());
    snapshot_at(&player_runtime, &connecting, "flow-connecting-0", 1_000);
    snapshot_at(&player_runtime, &connecting, "flow-connecting-1", 1_350);
}

// The connecting screen advances the installed loader without rebuilding text
// layouts or moving the status card.
#[test]
fn the_connecting_loader_animates_over_its_cached_layout() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping the_connecting_loader_animates_over_its_cached_layout: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let Some(images) = crate::ui_runtime::oreui_assets::load_optional_oreui_images()
        .filter(|images| !images.loading_frames.is_empty())
    else {
        eprintln!(
            "skipping the_connecting_loader_animates_over_its_cached_layout: fixture unavailable; requires the installed OreUI loading animation"
        );
        return;
    };
    let mut elapsed = 0_u64;
    let frames: Vec<_> = images
        .loading_frames
        .iter()
        .filter_map(|(key, millis)| {
            let start = elapsed;
            elapsed += u64::from(*millis);
            (*millis > 0).then(|| (start + u64::from(*millis) / 2, images.sprites[key]))
        })
        .collect();
    let first_frame = frames
        .first()
        .expect("installed loading animation has positive frame durations");
    let first_sprite = first_frame.1;
    let later_frame = frames
        .iter()
        .find(|(_, sprite)| *sprite != first_sprite)
        .expect("installed loading animation has distinct frames");
    let page = presentation.textures.dynamic_start() as u16;
    presentation.enable_oreui_originals(images).unwrap();
    let mut view = crate::menu::MenuView::new(true, "Test".into());
    view.connecting = true;
    view.message = Some("Connecting...".to_owned());
    let runtime = UiRuntime::new(1);
    let dpi = DpiScale::new(1.0).unwrap();
    let frame = |presentation: &mut super::super::UiPresentationRuntime, now_millis| {
        presentation.set_menu_view(Some(view.clone()));
        presentation
            .build(&player_runtime, &runtime, now_millis, [1280, 720], dpi)
            .unwrap()
    };
    frame(&mut presentation, 0);
    let cycle_start = (1_000 / elapsed + 1) * elapsed;
    let loader_bounds =
        |presentation: &super::super::UiPresentationRuntime,
         sprite: crate::ui_runtime::oreui_assets::OreUiSprite| {
            super::pack_harness::menu_nodes(presentation)
                .iter()
                .find_map(|node| match node.visual() {
                    UiVisual::Sprite {
                        texture_page, uv, ..
                    } if *texture_page == page + sprite.page && *uv == sprite.bounds => {
                        Some(node.bounds())
                    }
                    _ => None,
                })
                .expect("the selected installed loader frame is painted")
        };
    let text_layouts = |presentation: &super::super::UiPresentationRuntime| {
        super::pack_harness::menu_nodes(presentation)
            .iter()
            .filter_map(|node| match node.visual() {
                UiVisual::Text { layout, .. } => Some(layout.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let first = frame(&mut presentation, cycle_start + first_frame.0);
    let first_bounds = loader_bounds(&presentation, first_sprite);
    let first_text = text_layouts(&presentation);
    let later = frame(&mut presentation, cycle_start + later_frame.0);
    assert_eq!(first_bounds, loader_bounds(&presentation, later_frame.1));
    let later_text = text_layouts(&presentation);
    assert!(!first_text.is_empty());
    assert_eq!(first_text.len(), later_text.len());
    assert!(
        first_text
            .iter()
            .zip(&later_text)
            .all(|(first, later)| std::sync::Arc::ptr_eq(first, later)),
        "loader animation reuses unchanged text layouts"
    );
    let positions = |input: &render_model::UiRenderInput| {
        input
            .vertices
            .iter()
            .map(|vertex| vertex.position)
            .collect::<Vec<_>>()
    };
    let uvs = |input: &render_model::UiRenderInput| {
        input
            .vertices
            .iter()
            .map(|vertex| vertex.uv)
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(&first), positions(&later), "one layout");
    let pages = |input: &render_model::UiRenderInput| {
        input
            .batches
            .iter()
            .map(|batch| batch.texture_page)
            .collect::<Vec<_>>()
    };
    assert_ne!(
        (uvs(&first), pages(&first)),
        (uvs(&later), pages(&later)),
        "another frame of the installed loader"
    );
}

// The disconnect screen words the failure as vanilla does and offers OK, which
// leaves it for the menu.
#[test]
fn the_disconnect_screen_has_a_way_back() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping the_disconnect_screen_has_a_way_back: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let mut view = fixture_view(&dir);
    view.screen = MenuScreen::Play;
    view.disconnect_message =
        Some("network session failed: Bedrock session failed: Connection closed".to_owned());
    snapshot(&player_runtime, &view, "flow-disconnect");
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
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping the_server_list_scrolls_under_the_wheel: fixture unavailable; requires installed local carriers (make assets)"
        );
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
    let frame = |presentation: &mut super::super::UiPresentationRuntime, millis| {
        presentation.set_menu_view(Some(view.clone()));
        let input = presentation
            .build(&player_runtime, &runtime, millis, [1280, 720], dpi)
            .unwrap();
        (presentation.menu_hit_targets.clone(), input)
    };
    let row = |hits: &[(MenuAction, ui::UiRect)], index| {
        hits.iter()
            .find(|(action, _)| *action == MenuAction::SelectFeatured(index))
            .map(|(_, bounds)| *bounds)
    };
    let (before, input) = frame(&mut presentation, 0);
    super::snapshot::write(&input, "flow-servers-top");
    assert!(
        row(&before, 19).is_none(),
        "the last row starts below the fold"
    );
    let first = row(&before, 0).unwrap().min();
    let point = ui::UiPoint::new(first.x() + 4.0, first.y() + 4.0).unwrap();
    assert!(presentation.scroll_menu(point, -100.0, false));
    let (start, _) = frame(&mut presentation, 0);
    assert!(
        row(&start, 19).is_none(),
        "wheel input starts a transition instead of jumping"
    );
    let (after, input) = frame(&mut presentation, 200);
    super::snapshot::write(&input, "flow-servers-scrolled");
    assert!(row(&after, 19).is_some(), "the last row scrolled into view");
    assert!(row(&after, 0).is_none(), "the first row scrolled out");
}

// The settings screen's JSON-UI scroll views take the wheel like the OreUI lists.
#[test]
fn the_settings_panes_take_the_wheel() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping the_settings_panes_take_the_wheel: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
    std::fs::create_dir_all(&dir).unwrap();
    let mut view = fixture_view(&dir);
    view.screen = MenuScreen::Settings;
    let runtime = UiRuntime::new(1);
    let dpi = DpiScale::new(1.0).unwrap();
    presentation.set_menu_view(Some(view.clone()));
    presentation
        .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    let left = ui::UiPoint::new(200.0, 400.0).unwrap();
    assert!(presentation.scroll_menu(left, -40.0, false));
    presentation.set_menu_view(Some(view));
    let input = presentation
        .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    super::snapshot::write(&input, "flow-settings-scrolled");
}

// Writes PNGs of the local-world screens driven through the module (local only).
#[test]
fn snapshot_local_worlds() {
    let player_runtime = player_state::PlayerState::new(1);

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
        snapshot(&player_runtime, &view, name);
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
        max_players: None,
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
    snapshot_after(&player_runtime, &progress, "local-progress-download", 0, 40);
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
    menu.update(Input::SetBackend(Backend::Bds));
    menu.update(Input::SubmitCreate);
    shot(&menu, "local-docker-missing");
    menu.update(Input::Prompt(PromptButton::UseDragonfly));
    shot(&menu, "local-create-flat-only");
}

#[test]
fn review_artwork_fixtures_are_immutable_across_calls() {
    let dir = std::env::temp_dir().join(format!(
        "cinnabar-art-fixture-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let first = art(&dir, "same", [8, 8], [100, 0, 0]);
    let expected = std::fs::read(&first).unwrap();
    let second = art(&dir, "same", [8, 8], [0, 100, 0]);
    assert_ne!(first, second);
    assert_eq!(std::fs::read(first).unwrap(), expected);
    std::fs::remove_dir_all(dir).unwrap();
}

pub(super) use crate::test_support::{
    snapshot_menu as snapshot, snapshot_menu_after as snapshot_after,
    snapshot_menu_at as snapshot_at, snapshot_menu_vanilla as snapshot_vanilla,
};

pub(super) use crate::test_support::fixture_view;
use crate::test_support::play_flow::{art, server};

/// Captures matched font fixtures at desktop scales without contacting services.
#[test]
fn snapshot_shipped_font_routes_parity() {
    if std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR").is_none() {
        eprintln!("skipping font parity snapshots: missing CINNABAR_FORM_SNAPSHOT_DIR");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let base = fixture_view(dir.path());
    for (physical, dpi, suffix) in [
        ([2560, 1440], 2.0, ""),
        ([1280, 720], 1.0, "-scale2"),
        ([3024, 1964], 2.0, "-scale7"),
    ] {
        for (screen, name) in [
            (MenuScreen::Home, "home"),
            (MenuScreen::Play, "play"),
            (MenuScreen::Servers, "servers"),
            (MenuScreen::Settings, "settings"),
            (MenuScreen::Pause, "pause"),
        ] {
            let mut view = base.clone();
            view.screen = screen;
            view.over_world = screen == MenuScreen::Pause;
            if screen == MenuScreen::Servers {
                view.feeds.selected_featured = Some(0);
            }
            font_grid_snapshot(&view, physical, dpi, &format!("grid-{name}{suffix}"));
        }
    }
}

/// Captures supplied discovery text without loading account data or contacting a server.
#[test]
fn snapshot_shipped_font_motds() {
    let dir = tempfile::tempdir().unwrap();
    let base = fixture_view(dir.path());
    let Ok(path) = std::env::var("CINNABAR_FONT_MOTD_FIXTURE") else {
        eprintln!("skipping font MOTD snapshot: missing CINNABAR_FONT_MOTD_FIXTURE");
        return;
    };
    let rows: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut view = base;
    view.screen = MenuScreen::Servers;
    view.feeds.pings.clear();
    view.featured = rows
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, row)| {
            crate::test_support::play_flow::server(
                row["name"].as_str().unwrap(),
                &format!("fixture-{index}.invalid"),
                row["motd"].as_str().unwrap(),
                String::new(),
            )
        })
        .collect();
    font_grid_snapshot(&view, [2560, 1440], 2.0, "grid-motds");
}

/// Publishes and rasterizes a fixed offline screen at its physical size and DPI.
fn font_grid_snapshot(view: &crate::menu::MenuView, physical: [u32; 2], dpi: f32, name: &str) {
    let Some(mut presentation) = engine_presentation() else {
        return;
    };
    let runtime = super::pack_harness::menu_runtime();
    let player = player_state::PlayerState::new(1);
    presentation.sync_menu_artwork(crate::ui_runtime::presentation::menu_artwork::view_paths(
        view,
    ));
    presentation.finish_menu_artwork();
    let dpi = DpiScale::new(dpi).unwrap();
    for _ in 0..2 {
        presentation.set_menu_view(Some(view.clone()));
        presentation
            .build(&player, &runtime, 0, physical, dpi)
            .unwrap();
    }
    let input = presentation
        .build(&player, &runtime, 0, physical, dpi)
        .unwrap();
    super::snapshot::write(&input, name);
}
