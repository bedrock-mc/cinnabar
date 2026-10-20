use std::collections::BTreeMap;

use json_ui::Scalar;
use serde_json::json;
use ui::mod_panel::{PanelBundle, PanelPatch, Surface, SurfaceTransition, SurfaceValue};

use super::*;

/// A panel with a linear entrance, value transition and exit on one generic binding.
fn content(motion: bool) -> Panel {
    let mut panel = crate::test_support::sample_panel();
    panel.motion = motion;
    panel.surface = Some(Surface {
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{"type":"screen"}}).to_string(),
        bindings: BTreeMap::from([("#fade".into(), SurfaceValue::Number(1.0))]),
        transitions: BTreeMap::from([(
            "#fade".into(),
            SurfaceTransition {
                duration: 1.0,
                easing: Default::default(),
                from: Some(SurfaceValue::Number(0.0)),
                exit: Some(SurfaceValue::Number(0.0)),
            },
        )]),
        collections: Default::default(),
        previews: Default::default(),
    });
    panel
}

/// Publishes a bounded patch through the same replacement path as guest updates.
fn patch(ui: &mut ModUi, json: &str) {
    let panel = PanelPatch::parse(json)
        .unwrap()
        .apply(&ui.mod_panel.as_ref().unwrap().panel, &BTreeMap::new())
        .unwrap();
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
}

/// Samples and publishes the motion path used by painting without a renderer dependency.
fn sample(ui: &mut ModUi, now: f64) -> f64 {
    let current = ui.mod_panel.as_mut().unwrap();
    current.advance_motion(now, !ui.reduced_motion);
    current
        .motion
        .write(&current.panel, Arc::make_mut(&mut current.data));
    current.motion_data_dirty = false;
    match current.data.global_value("#fade") {
        Some(Scalar::Num(value)) => *value,
        other => panic!("expected numeric transition binding, got {other:?}"),
    }
}

/// Both directions retain the prepared route and settle existing feedback without a new entrance.
#[test]
fn motion_preference_snaps_feedback_and_resumes_only_future_transitions() {
    let panel = content(true);
    let bundle = Arc::new(PanelBundle {
        panel: panel.clone(),
        surfaces: Default::default(),
        control_schemas: Default::default(),
    });
    let prepared = prepared::PreparedPanelUi::prepare(bundle).unwrap();
    let mut ui = ModUi::default();
    let owner = PanelUiOwner::Environment(0);
    ui.set_prepared_panel_ui(owner, Some(prepared));
    ui.set_mod_panel(owner, Some(&panel)).unwrap();
    ui.set_mod_panel_open(true);
    let route = Arc::clone(ui.mod_panel.as_ref().unwrap().prepared.as_ref().unwrap());
    assert_eq!(sample(&mut ui, 1.0), 0.0);
    assert_eq!(sample(&mut ui, 1.5), 0.5);
    patch(&mut ui, r#"{"control_values":[[0,true]]}"#);
    {
        let current = ui.mod_panel.as_mut().unwrap();
        current.motion.scroll("list", 0.0, 30.0, 100.0);
        current
            .motion
            .advance(&current.panel, &mut current.view, Some(0), 1.5, true);
        current
            .motion
            .advance(&current.panel, &mut current.view, Some(0), 1.54, true);
        current
            .motion
            .write(&current.panel, Arc::make_mut(&mut current.data));
        assert!(current.motion.active());
    }
    patch(&mut ui, r#"{"motion":false}"#);
    let current = ui.mod_panel.as_ref().unwrap();
    assert!(Arc::ptr_eq(current.prepared.as_ref().unwrap(), &route));
    assert!(current.open);
    assert!(!current.motion.active());
    assert_eq!(current.view.scroll_offset("list"), 30.0);
    assert_eq!(current.data.global_value("#fade"), Some(&Scalar::Num(1.0)));
    assert_eq!(
        current.data.global_value("#row_0_toggle_progress"),
        Some(&Scalar::Num(1.0))
    );
    assert_eq!(
        current.data.global_value("#row_0_hover"),
        Some(&Scalar::Num(0.0))
    );
    assert!(!ui.mod_panel_motion_active());
    patch(&mut ui, r#"{"motion":true}"#);
    assert!(Arc::ptr_eq(
        ui.mod_panel.as_ref().unwrap().prepared.as_ref().unwrap(),
        &route
    ));
    assert_eq!(sample(&mut ui, 2.0), 1.0);
    assert!(!ui.mod_panel_motion_active());
    patch(&mut ui, r##"{"binding_changes":{"#fade":0.0}}"##);
    assert_eq!(sample(&mut ui, 2.0), 1.0);
    assert_eq!(sample(&mut ui, 2.5), 0.5);
    assert!(ui.mod_panel_motion_active());
    assert_eq!(sample(&mut ui, 3.0), 0.0);
    assert!(!ui.mod_panel_motion_active());
}

/// Disabled entrances, changed values and exits all complete immediately, even before a paint.
#[test]
fn disabled_panel_motion_never_retains_an_entrance_or_exit() {
    let mut ui = ModUi::default();
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&content(false)))
        .unwrap();
    ui.set_mod_panel_open(true);
    assert_eq!(
        ui.mod_panel.as_ref().unwrap().data.global_value("#fade"),
        Some(&Scalar::Num(1.0))
    );
    assert!(!ui.mod_panel_motion_active());
    // Enabling before the first paint must not replay the entrance just skipped.
    patch(&mut ui, r#"{"motion":true}"#);
    assert_eq!(sample(&mut ui, 1.0), 1.0);
    assert!(!ui.mod_panel_motion_active());
    patch(
        &mut ui,
        r##"{"motion":false,"binding_changes":{"#fade":0.75}}"##,
    );
    assert_eq!(sample(&mut ui, 1.1), 0.75);
    assert!(!ui.mod_panel_motion_active());
    ui.set_mod_panel_open(false);
    assert!(!ui.mod_panel_painting());
    assert!(!ui.mod_panel_motion_active());
    ui.set_mod_panel_open(true);
    assert_eq!(sample(&mut ui, 2.0), 0.75);
    assert!(!ui.mod_panel_motion_active());
    // A future genuine opening animates again once enabled while closed.
    ui.set_mod_panel_open(false);
    patch(&mut ui, r#"{"motion":true}"#);
    ui.set_mod_panel_open(true);
    assert_eq!(sample(&mut ui, 3.0), 0.0);
    assert!(ui.mod_panel_motion_active());
}

/// Disabling either preference during an exit removes the closed surface synchronously.
#[test]
fn disabling_motion_finishes_an_exit_without_another_frame() {
    for global in [false, true] {
        let mut ui = ModUi::default();
        ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&content(true)))
            .unwrap();
        ui.set_mod_panel_open(true);
        sample(&mut ui, 1.0);
        sample(&mut ui, 2.0);
        ui.set_mod_panel_open(false);
        assert!(ui.mod_panel_painting());
        assert_eq!(sample(&mut ui, 2.0), 1.0);
        assert_eq!(sample(&mut ui, 2.5), 0.5);
        if global {
            ui.configure_motion(false);
        } else {
            patch(&mut ui, r#"{"motion":false}"#);
        }
        assert!(!ui.mod_panel_open());
        assert!(!ui.mod_panel_painting());
        assert!(!ui.mod_panel_motion_active());
        assert!(!ui.mod_panel.as_ref().unwrap().motion.active());
    }
}

/// Panel motion cannot override the host preference, and re-enabling either avoids replay.
#[test]
fn screen_animation_preference_still_bounds_panel_motion() {
    let mut ui = ModUi::default();
    ui.configure_motion(false);
    ui.set_mod_panel(PanelUiOwner::Environment(0), Some(&content(true)))
        .unwrap();
    ui.set_mod_panel_open(true);
    assert_eq!(sample(&mut ui, 1.0), 1.0);
    assert!(!ui.mod_panel_motion_active());
    patch(&mut ui, r##"{"binding_changes":{"#fade":0.75}}"##);
    assert_eq!(sample(&mut ui, 1.1), 0.75);
    assert!(!ui.mod_panel_motion_active());
    ui.configure_motion(true);
    assert_eq!(sample(&mut ui, 1.2), 0.75);
    assert!(!ui.mod_panel_motion_active());
    patch(&mut ui, r##"{"binding_changes":{"#fade":0.25}}"##);
    assert_eq!(sample(&mut ui, 1.2), 0.75);
    assert_eq!(sample(&mut ui, 1.7), 0.5);
    ui.configure_motion(false);
    assert_eq!(
        ui.mod_panel.as_ref().unwrap().data.global_value("#fade"),
        Some(&Scalar::Num(0.25))
    );
    assert!(!ui.mod_panel_motion_active());
}
