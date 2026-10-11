use super::super::{snapshot, tests::mini_engine_presentation};
use super::tests::{frame, panel, point};
use super::*;
use serde_json::json;
use ui::mod_panel::{Surface, SurfaceValue};

/// Builds a generic extension surface with clipped controls and a bound content height.
fn scroll_panel() -> Panel {
    let mut panel = panel();
    panel.controls = ["first", "last"]
        .map(|id| Control::Button {
            id: id.into(),
            label: id.into(),
        })
        .into();
    let button = |index, y| json!({"type":"button","size":[90,20],"offset":[0,y],"anchor_from":"top_left","anchor_to":"top_left","button_mappings":[{"from_button_id":"button.menu_select","to_button_id":format!("mod.control:{index}"),"mapping_type":"pressed"}],"controls":[{"fill":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":["100%","100%"],"color":if index == 0 { [0.2,0.6,0.8,1.] } else { [0.2,0.8,0.4,1.] },"radius":2}},{"label":{"type":"label","size":[90,20],"text":if index == 0 { "First control" } else { "Last control" },"color":[1,1,1,1],"text_alignment":"center","anchor_from":"top_left","anchor_to":"top_left"}}]});
    let content = json!({"type":"panel","size":[90,200],"anchor_from":"top_left","anchor_to":"top_left",
        "bindings":[{"binding_name":"#content_height","binding_name_override":"#size_binding_y_absolute"}],
        "controls":[{"first":button(0,0)},{"last":button(1,180)}]});
    let viewport = json!({"type":"panel","size":[90,50],"anchor_from":"top_left","anchor_to":"top_left",
        "clips_children":true,"controls":[{"content":content}]});
    let track = json!({"type":"scroll_track","size":[10,50],"anchor_from":"top_left","anchor_to":"top_left",
        "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.scroll_track","mapping_type":"pressed"}]});
    let thumb = json!({"type":"scrollbar_box","size":[10,15],"draggable":"vertical","anchor_from":"top_left","anchor_to":"top_left"});
    let bar = json!({"type":"panel","size":[10,50],"offset":[90,0],"anchor_from":"top_left","anchor_to":"top_left",
        "controls":[{"track":track},{"box":thumb}]});
    let list = json!({"type":"scroll_view","size":[100,50],"offset":[20,20],"anchor_from":"top_left","anchor_to":"top_left",
        "scroll_view_port":"viewport","scroll_content":"content","scrollbar_track":"track","scrollbar_box":"box",
        "scroll_box_and_track_panel":"bar","scrollbar_track_button":"mod.scroll_track","scroll_speed":25,"always_handle_pointer":true,
        "controls":[{"viewport":viewport},{"bar":bar}]});
    panel.surface = Some(Surface {
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{"type":"screen","size":["100%","100%"],"controls":[{"list":list}]}}).to_string(),
        bindings: std::collections::BTreeMap::from([("#content_height".into(), SurfaceValue::Number(200.))]),
    });
    panel
}

/// Converts a panel's virtual coordinates to its actual window-logical input coordinates.
fn at(presentation: &UiPresentationRuntime, xy: [f64; 2]) -> [f32; 2] {
    let frame = presentation
        .form_presentation
        .mod_panel
        .as_ref()
        .unwrap()
        .frame
        .as_ref()
        .unwrap();
    [
        frame.origin[0] + xy[0] as f32 * frame.scale,
        frame.origin[1] + xy[1] as f32 * frame.scale,
    ]
}

/// Reads the visible first scroll view's layout result after a rendered frame.
fn offset(presentation: &UiPresentationRuntime) -> f64 {
    presentation
        .form_presentation
        .mod_panel
        .as_ref()
        .unwrap()
        .frame
        .as_ref()
        .unwrap()
        .report
        .scrolls
        .values()
        .next()
        .unwrap()
        .offset
}

#[test]
fn wheel_reaches_clipped_controls_and_content_resize_clamps_the_offset() {
    let mut presentation = mini_engine_presentation();
    let mut panel = scroll_panel();
    presentation.set_mod_panel(Some(&panel)).unwrap();
    presentation.set_mod_panel_open(true);
    let before = frame(&mut presentation, [1280, 720]);
    snapshot::write(&before, "personal-panel-scroll-before");
    let inside = at(&presentation, [65., 45.]);
    assert!(!presentation.scroll_mod_panel(at(&presentation, [5., 5.]), -20., false));
    assert!(presentation.scroll_mod_panel(inside, -20., false));
    let after = frame(&mut presentation, [1280, 720]);
    snapshot::write(&after, "personal-panel-scroll-after");
    assert_eq!(offset(&presentation), 150.);
    let last = point(&presentation, "mod.control:1", 0.5);
    assert_eq!(
        presentation.mod_panel_events(last, true, true)[0].id,
        "last"
    );
    panel
        .surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#content_height".into(), SurfaceValue::Number(50.));
    presentation.set_mod_panel(Some(&panel)).unwrap();
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 0.);
    panel
        .surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#content_height".into(), SurfaceValue::Number(200.));
    presentation.set_mod_panel(Some(&panel)).unwrap();
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 0.);
    presentation.set_mod_panel_open(false);
    assert!(!presentation.scroll_mod_panel(inside, -1., false));
}

#[test]
fn scrollbar_drag_applies_release_position_and_focus_loss_cancels_capture() {
    let mut presentation = mini_engine_presentation();
    presentation.set_mod_panel(Some(&scroll_panel())).unwrap();
    presentation.set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let thumb = at(&presentation, [115., 25.]);
    assert!(presentation.mod_panel_events(thumb, true, true).is_empty());
    let bottom = at(&presentation, [115., 70.]);
    assert!(
        presentation
            .mod_panel_events(bottom, false, false)
            .is_empty()
    );
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 150.);
    assert!(presentation.scroll_mod_panel(at(&presentation, [65., 45.]), 20., false));
    frame(&mut presentation, [1280, 720]);
    presentation.mod_panel_events(thumb, true, true);
    presentation.cancel_mod_panel_pointer_input();
    presentation.mod_panel_events(bottom, false, true);
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 0.);
}

#[test]
fn pixel_wheels_and_track_presses_use_the_drawn_panel_geometry() {
    let mut presentation = mini_engine_presentation();
    presentation.set_mod_panel(Some(&scroll_panel())).unwrap();
    presentation.set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let inside = at(&presentation, [65., 45.]);
    let scale = presentation
        .form_presentation
        .mod_panel
        .as_ref()
        .unwrap()
        .frame
        .as_ref()
        .unwrap()
        .scale;
    assert!(presentation.scroll_mod_panel(inside, -10. * f64::from(scale), true));
    assert!(presentation.scroll_mod_panel(inside, -10. * f64::from(scale), true));
    assert!(!presentation.scroll_mod_panel(inside, f64::NAN, true));
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 20.);
    let track = at(&presentation, [115., 69.]);
    assert!(presentation.mod_panel_events(track, true, true).is_empty());
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 150.);
}
