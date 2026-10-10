use std::sync::Arc;

use serde_json::json;

use {super::*, launcher::menu::MenuScreen};

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

fn in_world(
    player_runtime: &mut player_state::PlayerState,
    table: Arc<ScreenSettingsTable>,
) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime
        .publish_local_runtime_id(player_runtime, 1, 42)
        .unwrap();
    runtime.publish_inventory_authority(player_runtime, protocol::InventoryAuthority::Server);
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

#[test]
fn death_overlay_keeps_the_hud_visible_and_absorbs_gameplay_input() {
    let mut player = player_state::PlayerState::new(1);
    let table = Arc::new(ScreenSettingsTable(HashMap::from([
        (
            json_ui::HUD_SCREEN,
            settings(
                json!({"absorbs_input":false,"is_showing_menu":false,"render_only_when_topmost":true}),
            ),
        ),
        (
            "death.death_screen",
            settings(json!({"render_game_behind":false})),
        ),
    ])));
    let runtime = in_world(&mut player, table);
    let stack = runtime.scenes(&player, host(Some(MenuScreen::Death)));
    assert_eq!(
        stack.visible(false),
        [Scene::Gameplay, Scene::Hud, Scene::Menu(MenuScreen::Death)]
    );
    assert!(stack.top().unwrap().settings.absorbs_input);
    assert!(!stack.top().unwrap().settings.should_steal_mouse);
}

#[test]
fn credits_own_gameplay_input_until_the_session_bound_completion_is_sent() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = in_world(&mut player, vanilla_like_table());
    assert!(runtime.credits_mut().open(42, 8, 0));
    assert!(!runtime.gameplay_input(&player, None));
    assert!(!runtime.steals_mouse(&player, None));
    assert_eq!(
        runtime
            .scenes(&player, host(Some(MenuScreen::Pause)))
            .top()
            .unwrap()
            .key,
        Scene::Credits
    );
    runtime.credits_mut().skip(10);
    assert_eq!(
        runtime
            .credits_mut()
            .flush(Some(42), |_| Err(super::super::FormTransportError::Full)),
        Err(super::super::FormTransportError::Full)
    );
    assert!(!runtime.gameplay_input(&player, None));
    runtime.credits_mut().flush(Some(42), |_| Ok(())).unwrap();
    assert!(runtime.gameplay_input(&player, None));
}

// Only the HUD over the world: clicks reach gameplay and the mouse is captured.
#[test]
fn hud_only_routes_clicks_to_gameplay() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let runtime = in_world(&mut player_runtime, vanilla_like_table());
    assert!(runtime.gameplay_input(&player_runtime, None));
    assert!(!runtime.ui_focused(&player_runtime));
    assert!(runtime.steals_mouse(&player_runtime, None));
    assert_eq!(
        runtime.scenes(&player_runtime, host(None)).visible(false),
        [Scene::Gameplay, Scene::Hud]
    );
    let crosshair = settings(
        json!({"render_only_when_topmost": false, "absorbs_input": false,
        "is_showing_menu": false, "should_steal_mouse": false}),
    );
    let mut table = (*vanilla_like_table()).clone();
    table.0.insert(json_ui::CROSSHAIR_SCREEN, crosshair);
    let mut runtime = in_world(&mut player_runtime, Arc::new(table));
    assert!(
        runtime.steals_mouse(&player_runtime, None),
        "the crosshair overlay sits under the HUD"
    );
    runtime.open_chat(&mut player_runtime);
    assert_eq!(
        runtime.scenes(&player_runtime, host(None)).visible(false),
        [Scene::Gameplay, Scene::Crosshair, Scene::Chat]
    );
}

// An absorbing screen (inventory, chat, pause) takes clicks and frees the mouse.
#[test]
fn absorbing_screens_route_clicks_to_the_ui() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = in_world(&mut player_runtime, vanilla_like_table());
    runtime.toggle_inventory(&mut player_runtime);
    assert!(runtime.inventory_open());
    assert!(!runtime.gameplay_input(&player_runtime, None));
    assert!(!runtime.steals_mouse(&player_runtime, None));
    assert_eq!(
        runtime.scenes(&player_runtime, host(None)).visible(false),
        [Scene::Gameplay, Scene::Container]
    );
    runtime.close_inventory(&mut player_runtime);
    runtime.open_chat(&mut player_runtime);
    assert!(!runtime.gameplay_input(&player_runtime, None));
    runtime.close_chat();
    assert!(runtime.gameplay_input(&player_runtime, None));

    let paused = runtime.scenes(&player_runtime, host(Some(MenuScreen::Pause)));
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
    let mut player_runtime = player_state::PlayerState::new(1);

    let hud = settings(json!({"is_showing_menu": false, "should_steal_mouse": true}));
    let table = ScreenSettingsTable(HashMap::from([(json_ui::HUD_SCREEN, hud)]));
    let runtime = in_world(&mut player_runtime, Arc::new(table));
    assert!(!runtime.gameplay_input(&player_runtime, None));
}

// The launcher's menus have no world under them; its start screen takes input.
#[test]
fn launcher_menus_have_no_world_beneath() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let runtime = in_world(&mut player_runtime, vanilla_like_table());
    let launcher = SceneHost {
        over_world: false,
        ..host(Some(MenuScreen::Home))
    };
    let stack = runtime.scenes(&player_runtime, launcher);
    assert!(!stack.contains(Scene::Gameplay));
    assert_eq!(stack.visible(false), [Scene::Menu(MenuScreen::Home)]);
    assert!(!runtime.scenes(&player_runtime, launcher).steals_mouse());
}

// A health drop closes a top container whose screen asks for it.
#[test]
fn hurt_closes_a_top_container_that_asks() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = in_world(&mut player_runtime, vanilla_like_table());
    runtime.toggle_inventory(&mut player_runtime);
    runtime.note_player_hurt();
    runtime.close_scenes_on_player_hurt(&mut player_runtime, None);
    assert!(!runtime.inventory_open());
}

// The real carrier's HUD passes input through and a container absorbs it.
#[test]
fn carrier_settings_route_input_like_vanilla() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(carrier) = super::super::presentation::forms::pack_harness::carrier() else {
        eprintln!(
            "skipping carrier_settings_route_input_like_vanilla: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
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
    let mut runtime = in_world(&mut player_runtime, Arc::new(table));
    assert!(runtime.gameplay_input(&player_runtime, None));
    runtime.toggle_inventory(&mut player_runtime);
    assert!(!runtime.gameplay_input(&player_runtime, None));
    assert_eq!(
        runtime.scenes(&player_runtime, host(None)).closes_on_hurt(),
        None
    );
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
