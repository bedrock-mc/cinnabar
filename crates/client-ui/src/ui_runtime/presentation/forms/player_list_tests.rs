//! The held roster draws, follows roster changes and hides behind chat.

use std::sync::Arc;

use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::UiPresentationRuntime;
use ui::DpiScale;

fn names(count: usize) -> Vec<(Arc<str>, Arc<str>, [u8; 16])> {
    (0..count)
        .map(|index| {
            (
                Arc::from(format!("Player{index:02}")),
                Arc::from(""),
                [0; 16],
            )
        })
        .collect()
}

/// Supplies a HUD scene while retaining the deterministic in-memory carrier.
fn enable_empty_hud(presentation: &mut UiPresentationRuntime) {
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
}

#[test]
fn hold_release_roster_changes_and_chat_focus_drive_the_rendered_overlay() {
    let mut presentation = mini_engine_presentation();
    enable_empty_hud(&mut presentation);
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.player_roster.set_entries(names(2));
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
    runtime.player_roster.set_entries(names(3));
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
    frame(&mut presentation, &player, &runtime);
    assert!(presentation.screens.player_list.is_none());
}

#[test]
fn touch_toggle_survives_keyboard_release_and_resets_with_the_session() {
    let mut runtime = UiRuntime::new(1);
    runtime.toggle_touch_player_list();
    runtime.set_player_list_held(false);
    assert!(runtime.player_list_held());
    runtime.toggle_touch_player_list();
    assert!(!runtime.player_list_held());
    runtime.set_player_list_held(true);
    runtime.toggle_touch_player_list();
    runtime.toggle_touch_player_list();
    assert!(runtime.player_list_held());
    runtime.set_player_list_held(false);
    assert!(!runtime.player_list_held());
    runtime.toggle_touch_player_list();
    runtime.begin_session(2);
    assert!(!runtime.player_list_held());
}

#[test]
fn touch_roster_heading_stays_below_utility_buttons_on_first_paint() {
    for size in [[1280, 720], [1080, 1920]] {
        let mut presentation = mini_engine_presentation();
        enable_empty_hud(&mut presentation);
        let safe = ui::SafeArea::new(12., 8., 14., 6.).unwrap();
        presentation.set_safe_area(safe);
        presentation.enable_touch_controls(true);
        let mut runtime = UiRuntime::new(1);
        runtime.player_roster.set_entries(names(8));
        runtime.toggle_touch_player_list();
        let dpi = DpiScale::new(1.).unwrap();
        presentation
            .build(&player_state::PlayerState::new(1), &runtime, 0, size, dpi)
            .unwrap();
        let scale = ui_engine::text_metrics::TextMetrics::for_viewport(size, dpi, None).gui_scale;
        let chat = presentation
            .gameplay_touch_region(
                [
                    (safe.left() + (size[0] as f32 - safe.left() - safe.right()) / 2.)
                        / size[0] as f32,
                    (safe.top() + 12. * scale) / size[1] as f32,
                ],
                size.map(|v| v as f32),
            )
            .expect("chat button is reachable");
        assert_eq!(chat.hit_id, semantic_input::touch::CHAT);
        let nodes = &presentation.last_frame.as_ref().unwrap().nodes;
        let mut tree = ui::UiTree::new(nodes.clone()).unwrap();
        let viewport = ui::UiRect::new(
            ui::UiPoint::new(0., 0.).unwrap(),
            ui::UiPoint::new(size[0] as f32, size[1] as f32).unwrap(),
        )
        .unwrap();
        let laid = tree.layout(viewport, ui::UiScale::default(), safe).unwrap();
        let text_top = nodes
            .iter()
            .filter(|node| matches!(node.visual(), ui::UiVisual::Text { .. }))
            .map(|node| laid.bounds(node.id()).unwrap().min().y())
            .min_by(f32::total_cmp)
            .expect("roster has text");
        assert!(
            text_top >= chat.bounds.max[1] * size[1] as f32,
            "roster text at {text_top} overlaps the touch utility row"
        );
    }
}
