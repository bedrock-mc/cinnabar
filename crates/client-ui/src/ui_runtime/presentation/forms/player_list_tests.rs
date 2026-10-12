//! The held roster draws, follows roster changes and hides behind chat.

use std::sync::Arc;

use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::UiPresentationRuntime;
use ui::DpiScale;

fn names(count: usize) -> Vec<Arc<str>> {
    (0..count)
        .map(|index| Arc::from(format!("Player{index:02}")))
        .collect()
}

#[test]
fn hold_release_roster_changes_and_chat_focus_drive_the_rendered_overlay() {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&super::super::ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            (
                "ui/hud_screen.json".into(),
                br#"{"namespace":"hud","hud_screen":{"type":"screen","controls":[]}}"#.to_vec(),
            ),
        ]],
        ..Default::default()
    });
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.refresh_raw_text_identities(|_| None, names(2));
    let frame = |presentation: &mut UiPresentationRuntime,
                 player: &player_state::PlayerState,
                 runtime: &UiRuntime| {
        presentation
            .build(player, runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
            .unwrap()
    };
    let hidden = frame(&mut presentation, &player, &runtime);
    runtime.set_player_list_held(true);
    let shown = frame(&mut presentation, &player, &runtime);
    assert_ne!(hidden.vertices, shown.vertices);
    assert_eq!(shown, frame(&mut presentation, &player, &runtime));
    assert_eq!(
        presentation.screens.player_list.as_ref().unwrap().passes(),
        1
    );
    runtime.refresh_raw_text_identities(|_| None, names(3));
    let changed = frame(&mut presentation, &player, &runtime);
    assert_ne!(shown.vertices, changed.vertices);
    runtime.set_player_list_held(false);
    let released = frame(&mut presentation, &player, &runtime);
    assert_eq!(hidden.vertices, released.vertices);
    assert_eq!(hidden.indices, released.indices);
    runtime.set_player_list_held(true);
    runtime.open_chat(&mut player);
    let passes = presentation.screens.player_list.as_ref().unwrap().passes();
    frame(&mut presentation, &player, &runtime);
    assert_eq!(
        presentation.screens.player_list.as_ref().unwrap().passes(),
        passes
    );
    runtime.begin_session(2);
    assert!(!runtime.player_list_held());
    assert!(runtime.known_player_names().is_empty());
}
