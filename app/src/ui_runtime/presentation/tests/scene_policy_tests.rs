//! App launcher transitions feed synchronous JSON-UI scene policy.
use super::*;

use client_ui::ui_runtime::presentation::forms::ServerUiPack;
use {crate::menu::MenuRuntime, launcher::menu::MenuAction};

#[test]
fn java_chat_keeps_the_world_and_hud_but_absorbs_gameplay() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = client_ui::test_support::pack_harness::engine_presentation()
    else {
        eprintln!(
            "skipping java_chat_keeps_the_world_and_hud_but_absorbs_gameplay: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    let menu = MenuRuntime::new(false, 2, "Tester".into());
    let chat = client_ui::test_support::screen_settings(
        &presentation,
        client_ui::ui_runtime::presentation::forms::chat_screen::CHAT_SCREEN,
    );
    let hud = client_ui::test_support::screen_settings(&presentation, json_ui::HUD_SCREEN);
    assert!(chat.absorbs_input && chat.render_game_behind);
    assert!(!hud.absorbs_input && hud.renders(false));
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, &menu));
    assert!(presentation.absorbs_gameplay_input(&player_runtime, &runtime, &menu));
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![(
            "ui/hud_screen.json".into(),
            br#"{"namespace":"hud","hud_screen":{"render_only_when_topmost":true}}"#.to_vec(),
        )]],
        ..Default::default()
    });
    assert!(
        !client_ui::test_support::screen_settings(&presentation, json_ui::HUD_SCREEN)
            .renders(false)
    );
}

#[test]
fn retail_menu_defaults_and_server_visibility_override_share_the_resolved_root() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = client_ui::test_support::pack_harness::engine_presentation()
    else {
        eprintln!(
            "skipping retail_menu_defaults_and_server_visibility_override_share_the_resolved_root: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(1);
    let mut menu = MenuRuntime::new(false, 2, "Tester".into());
    menu.open_pause();
    let pause = client_ui::test_support::menu_settings(&presentation, &runtime, &menu.view());
    assert!(pause.absorbs_input && pause.render_game_behind && pause.render_only_when_topmost);
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, &menu));
    menu.activate(MenuAction::PauseSettings);
    let settings = client_ui::test_support::menu_settings(&presentation, &runtime, &menu.view());
    assert!(settings.absorbs_input && settings.render_game_behind);
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, &menu));
    assert!(presentation.absorbs_gameplay_input(&player_runtime, &runtime, &menu));
    let (namespace, control) = launcher::menu::SETTINGS_SCREEN.split_once('.').unwrap();
    let mut opaque_settings = serde_json::json!({"namespace": namespace});
    opaque_settings[control] = serde_json::json!({"render_game_behind": false});
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![(
            "ui/settings_screen.json".into(),
            serde_json::to_vec(&opaque_settings).unwrap(),
        )]],
        ..Default::default()
    });
    assert!(!presentation.renders_game_behind(&player_runtime, &runtime, &menu));
    assert!(presentation.absorbs_gameplay_input(&player_runtime, &runtime, &menu));
    presentation.set_server_ui_pack(&ServerUiPack::default());
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, &menu));
    menu.set_visible(false);
    menu.open_pause();
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![(
            "ui/pause_screen.json".into(),
            br#"{"namespace":"pause","pause_screen":{"render_game_behind":false}}"#.to_vec(),
        )]],
        ..Default::default()
    });
    assert!(!presentation.renders_game_behind(&player_runtime, &runtime, &menu));
    assert!(presentation.absorbs_gameplay_input(&player_runtime, &runtime, &menu));
}
