//! Closing editor previews and the live HUD share one presentation lifetime boundary.

use super::mod_hud_editor::panel;
use super::mod_widgets::{content, presentation};
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use mod_ui::PanelUiOwner;
use ui::DpiScale;

/// Paints a deterministic point in the editor's authored closing transition.
fn frame_at(p: &mut UiPresentationRuntime, millis: u64) {
    p.build(
        &player_state::PlayerState::new(1),
        &UiRuntime::new(1),
        millis,
        [1280, 720],
        DpiScale::new(1.).unwrap(),
    )
    .unwrap();
}

#[test]
fn closing_editor_previews_hide_live_widgets_and_handoff_on_the_completion_frame() {
    let mut p = presentation(false);
    let live = content();
    p.set_mod_hud(Some(&live)).unwrap();
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    let mut draft = live.clone();
    draft.cards[0].position = Some([0.7, 0.5]);
    draft.surface = Some(
        serde_json::from_value(serde_json::json!({
            "screen":"fixture.root",
            "document":serde_json::json!({"namespace":"fixture","root":{
                "type":"screen","size":["100%","100%"],"controls":[]
            }}).to_string(),
            "bindings":{"#alpha":1},
            "transitions":{"#alpha":{"duration":0.2,"exit":0}}
        }))
        .unwrap(),
    );
    p.mod_ui_mut().open_mod_hud_editor(&draft).unwrap();
    frame_at(&mut p, 1000);
    p.mod_ui_mut().set_mod_panel_open(false);
    let _ = p.mod_ui_mut().take_mod_hud_editor_result();
    for millis in [1100, 1200] {
        frame_at(&mut p, millis);
        assert!(!p.mod_ui().mod_panel_open() && p.mod_ui().mod_hud_editor_painting());
        assert!(!p.mod_hud_visible(&player_state::PlayerState::new(1), &UiRuntime::new(1)));
        assert_eq!(
            p.mod_ui().mod_widgets_state().unwrap().viewport,
            [0.; 2],
            "live cards must not receive a paint pass underneath a closing draft"
        );
    }
    frame_at(&mut p, 1400);
    assert!(!p.mod_ui().mod_hud_editor_painting());
    assert!(p.mod_hud_visible(&player_state::PlayerState::new(1), &UiRuntime::new(1)));
    assert!(
        p.mod_ui().mod_widgets_state().unwrap().viewport[0] > 0.,
        "the live HUD paints on the frame that completes the exit"
    );
}
