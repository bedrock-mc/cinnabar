use std::collections::BTreeMap;

use mod_ui::PanelUiOwner;
use serde_json::json;
use ui::{
    DpiScale,
    mod_panel::{Panel, Surface, SurfaceTransition, SurfaceValue, TransitionEasing},
};

use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use mod_ui::test_support::sample_panel as panel;

/// One button whose `#slide` offset eases in from `[0, 20]` and toward each new value over
/// a quarter second, and to `exit` when the panel closes.
fn sliding_panel(x: f64, exit: Option<f64>) -> Panel {
    let mut panel = panel();
    let button = json!({"type":"button","size":[40,20],"offset":[0,20],
        "anchor_from":"top_left","anchor_to":"top_left",
        "bindings":[{"binding_name":"#slide","binding_name_override":"#offset"}],
        "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.control:0","mapping_type":"pressed"}]});
    panel.surface = Some(Surface {
        collections: Default::default(),
        previews: Default::default(),
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{
            "type":"screen","size":["100%","100%"],"controls":[{"slider":button}]
        }})
        .to_string(),
        bindings: BTreeMap::from([("#slide".into(), SurfaceValue::Vector(vec![x, 20.0]))]),
        transitions: BTreeMap::from([(
            "#slide".into(),
            SurfaceTransition {
                duration: 0.25,
                easing: TransitionEasing::default(),
                from: Some(SurfaceValue::Vector(vec![0.0, 20.0])),
                exit: exit.map(|x| SurfaceValue::Vector(vec![x, 20.0])),
            },
        )]),
    });
    panel
}

/// Builds one frame at `millis` on the presentation clock.
fn frame_at(presentation: &mut UiPresentationRuntime, millis: u64) -> render_model::UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &UiRuntime::new(1),
            millis,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

/// The rendered button's left edge in logical surface pixels.
fn button_x(presentation: &UiPresentationRuntime) -> f64 {
    let state = presentation.mod_ui().mod_panel_state().unwrap();
    state
        .frame
        .unwrap()
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("mod.control:0"))
        .expect("rendered button")
        .rect
        .x
}

#[test]
fn surface_bindings_ease_in_on_open_and_retarget_from_their_displayed_value() {
    let mut presentation = mini_engine_presentation();
    let owner = PanelUiOwner::Environment(0);
    presentation
        .mod_ui_mut()
        .set_mod_panel(owner, Some(&sliding_panel(100.0, None)))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let moved = |presentation: &mut UiPresentationRuntime, millis, origin: f64| {
        frame_at(presentation, millis);
        button_x(presentation) - origin
    };

    frame_at(&mut presentation, 1_000);
    let origin = button_x(&presentation);
    assert!(presentation.mod_ui().mod_panel_motion_active());
    assert_eq!(moved(&mut presentation, 1_125, origin), 50.0);
    assert_eq!(moved(&mut presentation, 1_250, origin), 100.0);
    assert!(!presentation.mod_ui().mod_panel_motion_active());

    presentation
        .mod_ui_mut()
        .set_mod_panel(owner, Some(&sliding_panel(0.0, None)))
        .unwrap();
    assert_eq!(moved(&mut presentation, 1_375, origin), 100.0);
    assert_eq!(moved(&mut presentation, 1_500, origin), 50.0);
    presentation
        .mod_ui_mut()
        .set_mod_panel(owner, Some(&sliding_panel(100.0, None)))
        .unwrap();
    assert_eq!(moved(&mut presentation, 1_500, origin), 50.0);
    assert_eq!(moved(&mut presentation, 1_625, origin), 75.0);

    presentation.mod_ui_mut().set_mod_panel_open(false);
    presentation.mod_ui_mut().set_mod_panel_open(true);
    assert_eq!(moved(&mut presentation, 2_000, origin), 0.0);
    assert_eq!(moved(&mut presentation, 2_250, origin), 100.0);
    assert!(!presentation.mod_ui().mod_panel_motion_active());
}

#[test]
fn closing_eases_to_exit_values_with_screen_animations_off() {
    use launcher::menu::settings_options::{SettingId, SettingsOptions};

    let mut presentation = mini_engine_presentation();
    let owner = PanelUiOwner::Environment(0);
    presentation
        .mod_ui_mut()
        .set_mod_panel(owner, Some(&sliding_panel(100.0, Some(0.0))))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let moved = |presentation: &mut UiPresentationRuntime, millis, origin: f64| {
        frame_at(presentation, millis);
        button_x(presentation) - origin
    };
    frame_at(&mut presentation, 1_000);
    let origin = button_x(&presentation);
    assert_eq!(moved(&mut presentation, 1_250, origin), 100.0);

    presentation.mod_ui_mut().set_mod_panel_open(false);
    assert!(!presentation.mod_ui().mod_panel_open());
    assert!(presentation.mod_ui().mod_panel_motion_active());
    assert_eq!(moved(&mut presentation, 1_250, origin), 100.0);
    assert_eq!(moved(&mut presentation, 1_375, origin), 50.0);
    presentation.mod_ui_mut().set_mod_panel_open(true);
    assert_eq!(moved(&mut presentation, 1_375, origin), 50.0);
    assert_eq!(moved(&mut presentation, 1_500, origin), 75.0);
    presentation.mod_ui_mut().set_mod_panel_open(false);
    assert_eq!(moved(&mut presentation, 1_500, origin), 75.0);
    frame_at(&mut presentation, 1_800);
    assert!(!presentation.mod_ui().mod_panel_painting());
    assert!(!presentation.mod_ui().mod_panel_motion_active());
    let state = presentation.mod_ui().mod_panel_state().unwrap();
    assert!(state.frame.is_none());

    let mut options = SettingsOptions::default();
    options.set(SettingId::ScreenAnimations.index(), 0);
    presentation.set_chat_settings_snapshot((std::sync::Arc::new(options), None));
    presentation.mod_ui_mut().set_mod_panel_open(true);
    assert_eq!(moved(&mut presentation, 2_000, origin), 0.0);
    assert!(presentation.mod_ui().mod_panel_motion_active());
    assert_eq!(moved(&mut presentation, 2_125, origin), 50.0);
    assert_eq!(moved(&mut presentation, 2_250, origin), 100.0);
    presentation.mod_ui_mut().set_mod_panel_open(false);
    assert!(presentation.mod_ui().mod_panel_painting());
    assert_eq!(moved(&mut presentation, 2_250, origin), 100.0);
    assert_eq!(moved(&mut presentation, 2_375, origin), 50.0);
    frame_at(&mut presentation, 2_500);
    assert!(!presentation.mod_ui().mod_panel_painting());
}

/// Native mod feedback follows its own opt-in even when built-in screen motion is disabled.
#[test]
fn native_feedback_works_with_screen_animations_off() {
    use launcher::menu::settings_options::{SettingId, SettingsOptions};
    use ui::mod_panel::Control;
    let mut presentation = mini_engine_presentation();
    let mut options = SettingsOptions::default();
    options.set(SettingId::ScreenAnimations.index(), 0);
    presentation.set_chat_settings_snapshot((std::sync::Arc::new(options), None));
    let mut content = panel();
    content.motion = true;
    let owner = PanelUiOwner::Environment(0);
    presentation
        .mod_ui_mut()
        .set_mod_panel(owner, Some(&content))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let before = frame_at(&mut presentation, 1_000);
    if let Control::Toggle { value, .. } = &mut content.controls[0] {
        *value = true;
    }
    presentation
        .mod_ui_mut()
        .set_mod_panel(owner, Some(&content))
        .unwrap();
    let point = super::mod_panel::point(&presentation, "mod.control:0", 0.5);
    presentation
        .mod_ui_mut()
        .mod_panel_events(point, false, false);
    frame_at(&mut presentation, 1_000);
    assert!(presentation.mod_ui().mod_panel_motion_active());
    let middle = frame_at(&mut presentation, 1_050);
    assert_ne!(
        before.vertices, middle.vertices,
        "toggle and hover must visibly ease"
    );
    let settled = frame_at(&mut presentation, 1_200);
    assert!(!presentation.mod_ui().mod_panel_motion_active());
    assert_ne!(middle.vertices, settled.vertices);
    content.motion = false;
    if let Control::Toggle { value, .. } = &mut content.controls[0] {
        *value = false;
    }
    presentation
        .mod_ui_mut()
        .set_mod_panel(owner, Some(&content))
        .unwrap();
    frame_at(&mut presentation, 1_300);
    assert!(
        !presentation.mod_ui().mod_panel_motion_active(),
        "a mod can still disable its feedback"
    );
}
