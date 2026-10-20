//! Temporal pointer input through painted, rebound extension drag surfaces.

use std::{collections::BTreeMap, sync::Arc};

use mod_ui::{PanelUiOwner, PreparedPanelUi};
use serde_json::json;
use ui::mod_panel::{Control, Event, Panel, PanelBundle, Surface, SurfaceValue};

use super::super::super::snapshot;
use super::mod_panel::frame;
use crate::{
    test_support::mini_engine_presentation, ui_runtime::presentation::UiPresentationRuntime,
};

const OWNER: PanelUiOwner = PanelUiOwner::PacketSession(1);

/// Compares routed values with tolerance for normal floating-point slider stepping.
fn assert_events(events: &[Event], expected: &[(&str, f32)]) {
    assert_eq!(events.len(), expected.len(), "{events:?}");
    for (actual, (id, value)) in events.iter().zip(expected) {
        assert_eq!(actual.id, *id);
        assert!(
            (actual.value - value).abs() < 0.000_001,
            "{actual:?}; expected {value}"
        );
    }
}

/// Authors two root-offset sliders and a scalable handle whose position is a binding.
fn draggable() -> Panel {
    let mut panel = mod_ui::test_support::sample_panel();
    panel.reference_size = Some([600.0, 400.0]);
    panel.controls = [("x", 0.2), ("y", 0.25)]
        .into_iter()
        .map(|(id, value)| Control::Slider {
            id: id.into(),
            label: id.into(),
            value,
            min: 0.0,
            max: 1.0,
            step: 0.001,
        })
        .collect();
    panel.surface = Some(Surface {
        collections: Default::default(),
        previews: Default::default(),
        screen: "drag.root".into(),
        transitions: Default::default(),
        bindings: BTreeMap::from([("#position".into(), SurfaceValue::Vector(vec![96.0, 91.0]))]),
        document: json!({"namespace":"drag", "root":{
            "type":"screen", "size":[600,400], "controls":[{"handle":{
                "type":"button", "size":[80,24],
                "anchor_from":"top_left", "anchor_to":"top_left",
                "bindings":[{"binding_name":"#position","binding_name_override":"#offset"}],
                "cinnabar_transform":{"scale":1.5,"origin":[0,0]},
                "button_mappings":[{"from_button_id":"button.menu_select",
                    "to_button_id":"mod.drag:0:1","mapping_type":"pressed"}],
                "controls":[{"fill":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
                    "size":["100%","100%"],"radius":0,"color":[0,1,0,1]}}]
            }}]
        }})
        .to_string(),
    });
    panel
}

/// Converts authored root coordinates through the actual last frame's fit and origin.
fn pointer(presentation: &UiPresentationRuntime, logical: [f32; 2]) -> [f32; 2] {
    let state = presentation.mod_ui().mod_panel_state().unwrap();
    let frame = state.frame.unwrap();
    [
        frame.origin[0] + logical[0] * frame.scale,
        frame.origin[1] + logical[1] * frame.scale,
    ]
}

/// Publishes a fresh worker-prepared catalog atomically with its active packet panel.
fn publish(presentation: &mut UiPresentationRuntime, panel: &Panel) {
    let prepared = PreparedPanelUi::prepare(Arc::new(PanelBundle {
        panel: panel.clone(),
        surfaces: Default::default(),
        control_schemas: Default::default(),
    }))
    .unwrap();
    presentation
        .mod_ui_mut()
        .set_prepared_mod_panel(OWNER, prepared, panel)
        .unwrap();
}

/// Publishes a guest's value and geometry echo without changing its authored document.
fn echo(presentation: &mut UiPresentationRuntime, panel: &mut Panel, events: &[Event]) {
    for event in events {
        let control = panel
            .controls
            .iter_mut()
            .find(|control| control.id() == event.id)
            .unwrap();
        let Control::Slider { value, .. } = control else {
            unreachable!()
        };
        *value = event.value;
    }
    let values = panel
        .controls
        .iter()
        .zip([480.0, 364.0])
        .map(|(control, travel)| {
            let Control::Slider { value, .. } = control else {
                unreachable!()
            };
            f64::from(*value) * travel
        })
        .collect();
    panel
        .surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#position".into(), SurfaceValue::Vector(values));
    publish(presentation, panel);
    presentation.mod_ui_mut().set_mod_panel_open(true);
}

#[test]
fn relative_drag_survives_guest_echo_and_repaint_at_the_displayed_position() {
    for size in [[1280, 800], [854, 480]] {
        let mut panel = draggable();
        let mut presentation = mini_engine_presentation();
        publish(&mut presentation, &panel);
        presentation.mod_ui_mut().set_mod_panel_open(true);
        let initial = frame(&mut presentation, size);
        snapshot::write(
            &initial,
            &format!("native-relative-drag-before-{}x{}", size[0], size[1]),
        );
        // The scaled handle extends right of its layout box, and grabbing there must not jump.
        let grab = [200.0, 100.0];
        let at = pointer(&presentation, grab);
        assert_eq!(
            snapshot::rasterize(&initial)
                .get_pixel(at[0] as u32, at[1] as u32)
                .0,
            [0, 255, 0, 255]
        );
        assert_events(
            &presentation.mod_ui_mut().mod_panel_events(at, true, true),
            &[("x", 0.2), ("y", 0.25)],
        );
        let at = pointer(&presentation, [296.0, 170.0]);
        let events = presentation.mod_ui_mut().mod_panel_events(at, false, true);
        assert_events(&events, &[("x", 0.4), ("y", 0.442)]);
        echo(&mut presentation, &mut panel, &events);
        let moved = frame(&mut presentation, size);
        snapshot::write(
            &moved,
            &format!("native-relative-drag-during-{}x{}", size[0], size[1]),
        );
        assert_eq!(
            snapshot::rasterize(&moved)
                .get_pixel(at[0] as u32, at[1] as u32)
                .0,
            [0, 255, 0, 255]
        );
        assert_ne!(initial.vertices, moved.vertices);
        // The moving handle's new geometry does not move the captured pointer's origin.
        let at = pointer(&presentation, [344.0, 210.0]);
        let events = presentation.mod_ui_mut().mod_panel_events(at, false, true);
        assert_events(&events, &[("x", 0.5), ("y", 0.552)]);
        echo(&mut presentation, &mut panel, &events);
        frame(&mut presentation, size);
        assert!(
            presentation
                .mod_ui_mut()
                .mod_panel_events(at, false, true)
                .is_empty()
        );
        let outside = pointer(&presentation, [900.0, -400.0]);
        let released = presentation
            .mod_ui_mut()
            .mod_panel_events(outside, false, false);
        assert_eq!(
            released,
            [
                Event {
                    id: "x".into(),
                    value: 1.0
                },
                Event {
                    id: "y".into(),
                    value: 0.0
                }
            ]
        );
        echo(&mut presentation, &mut panel, &released);
        frame(&mut presentation, size);
        assert!(
            presentation
                .mod_ui_mut()
                .mod_panel_events(at, false, true)
                .is_empty(),
            "release ends capture"
        );
    }
}

#[test]
fn ordinary_button_above_a_drag_handle_receives_the_press() {
    let mut panel = draggable();
    panel.controls.push(Control::Button {
        id: "done".into(),
        label: "Done".into(),
    });
    let surface = panel.surface.as_mut().unwrap();
    let mut document: serde_json::Value = serde_json::from_str(&surface.document).unwrap();
    document["root"]["controls"]
        .as_array_mut()
        .unwrap()
        .push(json!({"toolbar":{
            "type":"button","offset":[96,91],"size":[80,24],
            "anchor_from":"top_left","anchor_to":"top_left",
            "button_mappings":[{"from_button_id":"button.menu_select",
                "to_button_id":"mod.control:2","mapping_type":"pressed"}]
        }}));
    surface.document = document.to_string();
    let mut presentation = mini_engine_presentation();
    publish(&mut presentation, &panel);
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [120.0, 100.0]);
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_events(at, true, true),
        [Event {
            id: "done".into(),
            value: 1.0
        }]
    );
    let at = pointer(&presentation, [200.0, 180.0]);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(at, false, true)
            .is_empty(),
        "the occluded handle never captures"
    );
}

#[test]
fn disabled_buttons_and_unmapped_modals_block_underlying_drag_capture() {
    for overlay in [
        json!({"type":"button","enabled":false,"button_mappings":[{
            "from_button_id":"button.menu_select","to_button_id":"mod.control:0","mapping_type":"pressed"}]}),
        json!({"type":"input_panel","modal":true}),
    ] {
        let mut panel = draggable();
        let surface = panel.surface.as_mut().unwrap();
        let mut document: serde_json::Value = serde_json::from_str(&surface.document).unwrap();
        let mut overlay = overlay;
        overlay["offset"] = json!([96, 91]);
        overlay["size"] = json!([80, 24]);
        overlay["anchor_from"] = json!("top_left");
        overlay["anchor_to"] = json!("top_left");
        document["root"]["controls"]
            .as_array_mut()
            .unwrap()
            .push(json!({"blocker":overlay}));
        surface.document = document.to_string();
        let mut presentation = mini_engine_presentation();
        publish(&mut presentation, &panel);
        presentation.mod_ui_mut().set_mod_panel_open(true);
        frame(&mut presentation, [1280, 800]);
        let at = pointer(&presentation, [120.0, 100.0]);
        assert!(
            presentation
                .mod_ui_mut()
                .mod_panel_events(at, true, true)
                .is_empty()
        );
        let at = pointer(&presentation, [200.0, 180.0]);
        assert!(
            presentation
                .mod_ui_mut()
                .mod_panel_events(at, false, true)
                .is_empty()
        );
    }
}

#[test]
fn repeated_absolute_routes_keep_the_grabbed_rectangle() {
    let mut panel = draggable();
    let surface = panel.surface.as_mut().unwrap();
    surface.document = json!({"namespace":"drag","root":{
        "type":"screen","size":["100%","100%"],"controls":[
            {"first":{"type":"button","offset":[20,20],"size":[100,100],
                "anchor_from":"top_left","anchor_to":"top_left",
                "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.vector:0:1","mapping_type":"pressed"}]}},
            {"second":{"type":"button","offset":[200,100],"size":[100,100],
                "anchor_from":"top_left","anchor_to":"top_left",
                "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.vector:0:1","mapping_type":"pressed"}]}}
        ]
    }}).to_string();
    let mut presentation = mini_engine_presentation();
    publish(&mut presentation, &panel);
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [225.0, 150.0]);
    assert_events(
        &presentation.mod_ui_mut().mod_panel_events(at, true, true),
        &[("x", 0.25), ("y", 0.5)],
    );
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [250.0, 175.0]);
    assert_events(
        &presentation.mod_ui_mut().mod_panel_events(at, false, true),
        &[("x", 0.5), ("y", 0.75)],
    );
}

#[test]
fn replacing_a_drag_document_releases_its_old_capture() {
    let mut panel = draggable();
    let mut presentation = mini_engine_presentation();
    publish(&mut presentation, &panel);
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [120.0, 100.0]);
    presentation.mod_ui_mut().mod_panel_events(at, true, true);
    let surface = panel.surface.as_mut().unwrap();
    let mut document: serde_json::Value = serde_json::from_str(&surface.document).unwrap();
    document["root"]["controls"][0]["handle"]["size"] = json!([120, 30]);
    surface.document = document.to_string();
    publish(&mut presentation, &panel);
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [200.0, 180.0]);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(at, false, true)
            .is_empty(),
        "replacement geometry requires its own press edge"
    );
}

#[test]
fn relative_axis_preserves_its_initial_value_across_a_rebound_handle() {
    let mut panel = draggable();
    let Control::Slider { value, .. } = &mut panel.controls[0] else {
        unreachable!()
    };
    *value = 0.2003;
    let surface = panel.surface.as_mut().unwrap();
    surface.document = surface
        .document
        .replace("mod.drag:0:1", "mod.drag_axis:0:x");
    let mut presentation = mini_engine_presentation();
    publish(&mut presentation, &panel);
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [200.0, 100.0]);
    assert_events(
        &presentation.mod_ui_mut().mod_panel_events(at, true, true),
        &[("x", 0.2003)],
    );
    let at = pointer(&presentation, [320.0, 190.0]);
    let events = presentation.mod_ui_mut().mod_panel_events(at, false, true);
    assert_events(&events, &[("x", 0.4)]);
    echo(&mut presentation, &mut panel, &events);
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [380.0, 20.0]);
    assert_events(
        &presentation.mod_ui_mut().mod_panel_events(at, false, false),
        &[("x", 0.5)],
    );
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(at, false, true)
            .is_empty()
    );
}

#[test]
fn relative_axis_uses_the_declared_reference_extent_for_a_scale_handle() {
    let mut panel = draggable();
    let surface = panel.surface.as_mut().unwrap();
    surface.document = surface
        .document
        .replace("mod.drag:0:1", "mod.drag_axis:0:x:112");
    let mut presentation = mini_engine_presentation();
    publish(&mut presentation, &panel);
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [200.0, 100.0]);
    assert_events(
        &presentation.mod_ui_mut().mod_panel_events(at, true, true),
        &[("x", 0.2)],
    );
    let at = pointer(&presentation, [222.4, 150.0]);
    let events = presentation.mod_ui_mut().mod_panel_events(at, false, true);
    assert_events(&events, &[("x", 0.4)]);
    echo(&mut presentation, &mut panel, &events);
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [233.6, 180.0]);
    assert_events(
        &presentation.mod_ui_mut().mod_panel_events(at, false, false),
        &[("x", 0.5)],
    );
}

#[test]
fn bound_relative_axis_extent_is_captured_until_release_and_resampled_on_the_next_grab() {
    let mut panel = draggable();
    let surface = panel.surface.as_mut().unwrap();
    surface.document = surface
        .document
        .replace("mod.drag:0:1", "mod.drag_axis:0:x:#resize_extent");
    surface
        .bindings
        .insert("#resize_extent".into(), SurfaceValue::Number(112.0));
    let mut presentation = mini_engine_presentation();
    publish(&mut presentation, &panel);
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let initial = frame(&mut presentation, [1280, 800]);
    snapshot::write(&initial, "native-bound-extent-before");
    let at = pointer(&presentation, [200.0, 100.0]);
    assert_events(
        &presentation.mod_ui_mut().mod_panel_events(at, true, true),
        &[("x", 0.2)],
    );
    let at = pointer(&presentation, [222.4, 100.0]);
    let events = presentation.mod_ui_mut().mod_panel_events(at, false, true);
    assert_events(&events, &[("x", 0.4)]);
    // A newly prepared catalog and a new extent do not alter this gesture's sensitivity.
    panel
        .surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#resize_extent".into(), SurfaceValue::Number(224.0));
    echo(&mut presentation, &mut panel, &events);
    frame(&mut presentation, [1280, 800]);
    let at = pointer(&presentation, [233.6, 100.0]);
    let events = presentation.mod_ui_mut().mod_panel_events(at, false, true);
    assert_events(&events, &[("x", 0.5)]);
    echo(&mut presentation, &mut panel, &events);
    let moved = frame(&mut presentation, [1280, 800]);
    snapshot::write(&moved, "native-bound-extent-during");
    assert_ne!(initial.vertices, moved.vertices);
    let at = pointer(&presentation, [244.8, 100.0]);
    let events = presentation.mod_ui_mut().mod_panel_events(at, false, false);
    assert_events(&events, &[("x", 0.6)]);
    echo(&mut presentation, &mut panel, &events);
    frame(&mut presentation, [1280, 800]);
    // The next grab uses the updated extent, not the preceding gesture's captured value.
    let at = pointer(&presentation, [300.0, 100.0]);
    assert_events(
        &presentation.mod_ui_mut().mod_panel_events(at, true, true),
        &[("x", 0.6)],
    );
    let at = pointer(&presentation, [322.4, 100.0]);
    assert_events(
        &presentation.mod_ui_mut().mod_panel_events(at, false, false),
        &[("x", 0.7)],
    );
}
