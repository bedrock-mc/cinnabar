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
/// a quarter second.
fn sliding_panel(x: f64) -> Panel {
    let mut panel = panel();
    let button = json!({"type":"button","size":[40,20],"offset":[0,20],
        "anchor_from":"top_left","anchor_to":"top_left",
        "bindings":[{"binding_name":"#slide","binding_name_override":"#offset"}],
        "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.control:0","mapping_type":"pressed"}]});
    panel.surface = Some(Surface {
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
            },
        )]),
    });
    panel
}

/// Builds one frame at `millis` on the presentation clock.
fn frame_at(presentation: &mut UiPresentationRuntime, millis: u64) {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &UiRuntime::new(1),
            millis,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
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
        .set_mod_panel(owner, Some(&sliding_panel(100.0)))
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
        .set_mod_panel(owner, Some(&sliding_panel(0.0)))
        .unwrap();
    assert_eq!(moved(&mut presentation, 1_375, origin), 100.0);
    assert_eq!(moved(&mut presentation, 1_500, origin), 50.0);
    presentation
        .mod_ui_mut()
        .set_mod_panel(owner, Some(&sliding_panel(100.0)))
        .unwrap();
    assert_eq!(moved(&mut presentation, 1_500, origin), 50.0);
    assert_eq!(moved(&mut presentation, 1_625, origin), 75.0);

    presentation.mod_ui_mut().set_mod_panel_open(false);
    presentation.mod_ui_mut().set_mod_panel_open(true);
    assert_eq!(moved(&mut presentation, 2_000, origin), 0.0);
    assert_eq!(moved(&mut presentation, 2_250, origin), 100.0);
    assert!(!presentation.mod_ui().mod_panel_motion_active());
}
