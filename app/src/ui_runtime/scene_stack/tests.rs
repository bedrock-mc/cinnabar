use std::sync::Arc;

use serde_json::json;

use super::*;

fn settings(value: serde_json::Value) -> ScreenSettings {
    let serde_json::Value::Object(map) = value else {
        panic!("object");
    };
    ScreenSettings::from_properties(&map.into_iter().collect())
}

/// The vanilla HUD's declared settings; every other screen takes the defaults.
fn vanilla_like_table() -> Arc<ScreenSettingsTable> {
    let hud = settings(json!({"is_showing_menu": false, "should_steal_mouse": true,
        "low_frequency_rendering": true, "absorbs_input": false}));
    let chest = settings(json!({"close_on_player_hurt": true}));
    Arc::new(ScreenSettingsTable(HashMap::from([
        (json_ui::HUD_SCREEN, hud),
        ("crafting.inventory_screen", chest),
    ])))
}

fn in_world(table: Arc<ScreenSettingsTable>) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime.publish_local_runtime_id(1, 42).unwrap();
    runtime.publish_inventory_authority(protocol::InventoryAuthority::Server);
    runtime.set_screen_settings(table);
    runtime
}

fn host(menu: Option<MenuScreen>) -> SceneHost {
    SceneHost {
        menu,
        over_world: true,
        loading: false,
    }
}

// Only the HUD over the world: clicks reach gameplay and the mouse is captured.
#[test]
fn hud_only_routes_clicks_to_gameplay() {
    let runtime = in_world(vanilla_like_table());
    assert!(runtime.gameplay_input(None));
    assert!(!runtime.ui_focused());
    assert!(runtime.steals_mouse(None));
    assert_eq!(
        runtime.scenes(host(None)).visible(false),
        [Scene::Gameplay, Scene::Hud]
    );
    let crosshair = settings(
        json!({"render_only_when_topmost": false, "absorbs_input": false,
        "is_showing_menu": false, "should_steal_mouse": false}),
    );
    let mut table = (*vanilla_like_table()).clone();
    table.0.insert(json_ui::CROSSHAIR_SCREEN, crosshair);
    let mut runtime = in_world(Arc::new(table));
    assert!(
        runtime.steals_mouse(None),
        "the crosshair overlay sits under the HUD"
    );
    runtime.open_chat();
    assert_eq!(
        runtime.scenes(host(None)).visible(false),
        [Scene::Gameplay, Scene::Crosshair, Scene::Chat]
    );
}

// An absorbing screen (inventory, chat, pause) takes clicks and frees the mouse.
#[test]
fn absorbing_screens_route_clicks_to_the_ui() {
    let mut runtime = in_world(vanilla_like_table());
    runtime.toggle_inventory();
    assert!(runtime.inventory_open());
    assert!(!runtime.gameplay_input(None));
    assert!(!runtime.steals_mouse(None));
    assert_eq!(
        runtime.scenes(host(None)).visible(false),
        [Scene::Gameplay, Scene::Container]
    );
    runtime.close_inventory();
    runtime.open_chat();
    assert!(!runtime.gameplay_input(None));
    runtime.close_chat();
    assert!(runtime.gameplay_input(None));

    let paused = runtime.scenes(host(Some(MenuScreen::Pause)));
    assert!(!paused.receives_input(Scene::Gameplay));
    assert!(!paused.steals_mouse());
    assert_eq!(
        paused.visible(false),
        [Scene::Gameplay, Scene::Menu(MenuScreen::Pause)]
    );
}

// A pack that makes the HUD absorb input takes clicks from gameplay, as vanilla would.
#[test]
fn pack_declared_absorbing_hud_blocks_gameplay() {
    let hud = settings(json!({"is_showing_menu": false, "should_steal_mouse": true}));
    let table = ScreenSettingsTable(HashMap::from([(json_ui::HUD_SCREEN, hud)]));
    let runtime = in_world(Arc::new(table));
    assert!(!runtime.gameplay_input(None));
}

// The launcher's menus have no world under them; its start screen takes input.
#[test]
fn launcher_menus_have_no_world_beneath() {
    let runtime = in_world(vanilla_like_table());
    let launcher = SceneHost {
        over_world: false,
        ..host(Some(MenuScreen::Home))
    };
    let stack = runtime.scenes(launcher);
    assert!(!stack.contains(Scene::Gameplay));
    assert_eq!(stack.visible(false), [Scene::Menu(MenuScreen::Home)]);
    assert!(!runtime.scenes(launcher).steals_mouse());
}

// A health drop closes a top container whose screen asks for it.
#[test]
fn hurt_closes_a_top_container_that_asks() {
    let mut runtime = in_world(vanilla_like_table());
    runtime.toggle_inventory();
    runtime.note_player_hurt();
    let mut app = bevy::app::App::new();
    app.insert_resource(runtime)
        .add_systems(bevy::app::Update, close_scenes_on_player_hurt);
    app.update();
    assert!(!app.world().resource::<UiRuntime>().inventory_open());
}

// The real carrier's HUD passes input through and a container absorbs it.
#[test]
#[ignore = "requires installed local carriers (make assets)"]
fn carrier_settings_route_input_like_vanilla() {
    let carrier = super::super::presentation::forms::pack_harness::carrier()
        .expect("required offline fixture; see the ignore reason");
    let catalog = Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .unwrap();
    let table = ScreenSettingsTable::for_catalog(&catalog, &Context::retail(true));
    let hud = table
        .get(json_ui::HUD_SCREEN)
        .expect("the HUD screen resolves");
    assert!(!hud.absorbs_input && hud.should_steal_mouse);
    let mut runtime = in_world(Arc::new(table));
    assert!(runtime.gameplay_input(None));
    runtime.toggle_inventory();
    assert!(!runtime.gameplay_input(None));
    assert_eq!(runtime.scenes(host(None)).closes_on_hurt(), None);
}

// A pushed scene enters with a push fade; the one it covered re-enters with a pop fade.
#[test]
fn scene_clocks_time_entrance_transitions() {
    let mut clocks = SceneClocks::default();
    clocks.observe(&[], 0.0);
    clocks.observe(&[Scene::Gameplay, Scene::Hud], 1.0);
    assert_eq!(
        clocks.clocks(Scene::Hud).get("screen.entrance_push"),
        Some(&1.0)
    );
    clocks.observe(
        &[Scene::Gameplay, Scene::Hud, Scene::Menu(MenuScreen::Pause)],
        2.0,
    );
    let pause = clocks.clocks(Scene::Menu(MenuScreen::Pause));
    assert_eq!(pause.get("screen.entrance_push"), Some(&2.0));
    clocks.observe(&[Scene::Gameplay, Scene::Hud], 3.0);
    assert_eq!(
        clocks.clocks(Scene::Hud).get("screen.entrance_pop"),
        Some(&3.0)
    );
    assert!(clocks.clocks(Scene::Menu(MenuScreen::Pause)).is_empty());
}

// Switching between tabs of one vanilla screen is not a new entrance.
#[test]
fn menu_tabs_share_one_entrance() {
    let mut clocks = SceneClocks::default();
    clocks.observe(&[], 0.0);
    clocks.observe(&[Scene::Menu(MenuScreen::Play)], 1.0);
    clocks.observe(&[Scene::Menu(MenuScreen::Servers)], 2.0);
    let servers = clocks.clocks(Scene::Menu(MenuScreen::Servers));
    assert_eq!(servers.get("screen.entrance_push"), Some(&1.0));
}
