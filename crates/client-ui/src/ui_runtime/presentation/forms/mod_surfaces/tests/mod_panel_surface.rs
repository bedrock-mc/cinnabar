use mod_ui::PanelUiOwner;
use std::sync::Arc;

use mod_ui::test_support::panel_same_shape;
use serde_json::json;
use ui::{
    FONT_DESIGN_PIXEL_TEXELS,
    mod_panel::{Control, Event, Panel, Surface, SurfaceValue},
};

use super::super::super::snapshot;
use super::mod_panel::{frame, point};
use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::presentation::UiPresentationRuntime;
use mod_ui::test_support::sample_panel as panel;

/// Declares native control routes through an extension-owned catalog.
fn authored() -> Panel {
    let mut panel = panel();
    let button = |action: &str, x: f64| {
        json!({"type":"button", "size":[70,20],"offset":[x,20],
        "bindings":[{"binding_name":"(#surface_width > 1)","binding_name_override":"#visible"}],
        "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":action,"mapping_type":"pressed"}]})
    };
    panel.surface = Some(Surface {
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{
            "type":"screen","size":["100%","100%"],"controls":[
                {"color":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[280,45],"offset":[15,15],"radius":2,"color":[1,0,0,1],
                    "bindings":[{"binding_name":"#surface_color","binding_name_override":"#color"}]}},
                {"toggle":button("mod.control:0",20.)},
                {"number":button("mod.edit:1",100.)},
                {"choice":button("mod.control:2",180.)}
            ]
        }}).to_string(),
        bindings: std::collections::BTreeMap::from([("#surface_color".into(), SurfaceValue::Vector(vec![1.,0.,0.,1.]))]),
    });
    panel
}

#[test]
fn custom_geometry_routes_toggles_and_native_number_and_choice_editors() {
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&authored()))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let toggle = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(toggle, true, true),
        [Event {
            id: "enabled".into(),
            value: 1.0
        }]
    );
    frame(&mut presentation, [1280, 720]);
    let number = point(&presentation, "mod.edit:1", 0.5);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(number, true, true)
            .is_empty()
    );
    presentation
        .mod_ui_mut()
        .mod_panel_key("Digit4", Some("42"));
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_key("Enter", None),
        [Event {
            id: "strength".into(),
            value: 42.0
        }]
    );
    frame(&mut presentation, [1280, 720]);
    let choice = point(&presentation, "mod.control:2", 0.5);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(choice, true, true)
            .is_empty()
    );
    frame(&mut presentation, [1280, 720]);
    let option = point(&presentation, "mod.option:1", 0.5);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(option, true, true),
        [Event {
            id: "mode".into(),
            value: 1.0
        }]
    );
}

#[test]
fn changed_surface_bindings_reuse_the_catalog_and_respect_host_dimensions() {
    let mut presentation = mini_engine_presentation();
    let mut panel = authored();
    panel
        .surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#surface_width".into(), SurfaceValue::Number(1.));
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let before = frame(&mut presentation, [1280, 720]);
    let catalog = Arc::clone(
        presentation
            .mod_ui()
            .mod_panel_state()
            .unwrap()
            .catalog
            .unwrap(),
    );
    panel.surface.as_mut().unwrap().bindings.insert(
        "#surface_color".into(),
        SurfaceValue::Vector(vec![0., 1., 0., 1.]),
    );
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    let after = frame(&mut presentation, [1280, 720]);
    let retained = presentation.mod_ui().mod_panel_state().unwrap();
    assert!(Arc::ptr_eq(&catalog, retained.catalog.unwrap()));
    assert!(before.vertices != after.vertices);
    let changed = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(changed, true, true)[0]
            .id,
        "enabled"
    );
}

/// First-use routes arrive resolved, and a return reuses unchanged catalog and layout state.
#[test]
fn prepared_panel_routes_reuse_resolution_and_layout_on_return() {
    let initial = authored();
    let mut second = initial.clone();
    let mut document: serde_json::Value =
        serde_json::from_str(&second.surface.as_ref().unwrap().document).unwrap();
    document["screen"]["controls"][0]["color"]["radius"] = json!(7);
    second.surface.as_mut().unwrap().document = document.to_string();
    let bundle = ui::mod_panel::PanelBundle::parse(
        &json!({"panel":initial,"surfaces":{"other":second.surface}}).to_string(),
    )
    .unwrap();
    let prepared = mod_ui::PreparedPanelUi::prepare(Arc::new(bundle)).unwrap();
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_prepared_panel_ui(mod_ui::PanelUiOwner::Environment(0), Some(prepared));
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&initial))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let before = presentation.mod_ui().mod_panel_state().unwrap();
    let catalog = Arc::clone(before.catalog.unwrap());
    let passes = before.passes;
    assert_eq!(before.resolves, 0);
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&second))
        .unwrap();
    frame(&mut presentation, [1280, 720]);
    assert_eq!(presentation.mod_ui().mod_panel_state().unwrap().resolves, 0);
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&initial))
        .unwrap();
    frame(&mut presentation, [1280, 720]);
    let returned = presentation.mod_ui().mod_panel_state().unwrap();
    assert!(Arc::ptr_eq(&catalog, returned.catalog.unwrap()));
    assert_eq!(returned.passes, passes);
    assert_eq!(returned.resolves, 0);
}

/// Legacy schema or wrapper changes use validation and cannot reuse a differently prepared route.
#[test]
fn prepared_panel_falls_back_for_changed_wrapper_facts() {
    let initial = authored();
    let bundle =
        ui::mod_panel::PanelBundle::parse(&json!({"panel":initial,"surfaces":{}}).to_string())
            .unwrap();
    let prepared = mod_ui::PreparedPanelUi::prepare(Arc::new(bundle)).unwrap();
    let mut variants = Vec::new();
    let mut changed = initial.clone();
    changed.dark = !changed.dark;
    variants.push(changed);
    let mut changed = initial.clone();
    changed.title = "Another title".into();
    variants.push(changed);
    let mut changed = initial.clone();
    changed.toggle_key = "F8".into();
    variants.push(changed);
    let mut changed = initial.clone();
    changed.theme = ui::mod_panel::Theme::Monochrome;
    variants.push(changed);
    let mut changed = initial.clone();
    changed.style = ui::mod_panel::Style::Compact;
    variants.push(changed);
    let mut changed = initial.clone();
    changed.reference_size = Some([300.0, 200.0]);
    variants.push(changed);
    for changed in variants {
        let mut presentation = mini_engine_presentation();
        presentation.mod_ui_mut().set_prepared_panel_ui(
            mod_ui::PanelUiOwner::Environment(0),
            Some(Arc::clone(&prepared)),
        );
        presentation
            .mod_ui_mut()
            .set_mod_panel(PanelUiOwner::Environment(0), Some(&initial))
            .unwrap();
        presentation.mod_ui_mut().set_mod_panel_open(true);
        frame(&mut presentation, [1280, 720]);
        let catalog = Arc::clone(
            presentation
                .mod_ui()
                .mod_panel_state()
                .unwrap()
                .catalog
                .unwrap(),
        );
        presentation
            .mod_ui_mut()
            .set_mod_panel(PanelUiOwner::Environment(0), Some(&changed))
            .unwrap();
        frame(&mut presentation, [1280, 720]);
        let state = presentation.mod_ui().mod_panel_state().unwrap();
        assert!(!Arc::ptr_eq(&catalog, state.catalog.unwrap()));
        assert_eq!(state.resolves, 1);
    }
}

#[test]
fn reference_size_keeps_bottom_control_visible_and_clickable_in_small_viewports() {
    let mut panel = panel();
    panel.reference_size = Some([495.0, 304.0]);
    panel.surface = Some(Surface {
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{
            "type":"screen","size":["100%","100%"],"controls":[{"dialog":{
                "type":"panel","size":[495,304],"anchor_from":"center","anchor_to":"center",
                "controls":[{"bottom":{
                    "type":"button","size":[100,20],"offset":[380,276],
                    "anchor_from":"top_left","anchor_to":"top_left",
                    "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.control:3","mapping_type":"pressed"}],
                    "controls":[{"fill":{
                        "type":"custom","renderer":"cinnabar_rounded_rectangle",
                        "size":["100%","100%"],"radius":0,"color":[0,1,0,1]
                    }}]
                }}]
            }}]
        }}).to_string(),
        bindings: Default::default(),
    });
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    for size in [[854, 480], [1920, 1280]] {
        let rendered = frame(&mut presentation, size);
        let at = point(&presentation, "mod.control:3", 0.5);
        let frame = presentation
            .mod_ui()
            .mod_panel_state()
            .unwrap()
            .frame
            .unwrap();
        let hit = frame
            .hits
            .iter()
            .find(|hit| hit.pressed.as_deref() == Some("mod.control:3"))
            .unwrap();
        let right = frame.origin[0] + (hit.rect.x + hit.rect.w) as f32 * frame.scale;
        let bottom = frame.origin[1] + (hit.rect.y + hit.rect.h) as f32 * frame.scale;
        assert!(at[0] > 0.0 && at[1] > 0.0 && right <= size[0] as f32 && bottom <= size[1] as f32);
        if size == [854, 480] {
            assert!(frame.scale < FONT_DESIGN_PIXEL_TEXELS as f32);
        } else {
            assert_eq!(frame.scale, FONT_DESIGN_PIXEL_TEXELS as f32);
        }
        let image = snapshot::rasterize(&rendered);
        assert_eq!(
            image.get_pixel(at[0] as u32, at[1] as u32).0,
            [0, 255, 0, 255]
        );
        assert_eq!(
            presentation.mod_ui_mut().mod_panel_events(at, true, true),
            [Event {
                id: "binding".into(),
                value: 1.0
            }]
        );
        presentation.mod_ui_mut().mod_panel_events(at, false, false);
    }
    let resized = panel.clone();
    panel.reference_size = Some([600.0, 400.0]);
    assert!(!panel_same_shape(&resized, &panel));
}

#[test]
fn custom_panel_buttons_render_captured_press_and_clear_feedback_on_release_and_cancel() {
    let mut panel = panel();
    panel.controls = vec![Control::Button {
        id: "continue".into(),
        label: "Continue".into(),
    }];
    let state = |color| {
        json!({"type":"custom","renderer":"cinnabar_rounded_rectangle",
            "size":["100%","100%"],"radius":0,"color":color})
    };
    panel.surface = Some(Surface {
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{
            "type":"panel","size":["100%","100%"],"controls":[{"continue":{
                "type":"button","size":[80,24],"offset":[100,80],
                "anchor_from":"top_left","anchor_to":"top_left",
                "default_control":"normal","hover_control":"hover","pressed_control":"pressed",
                "button_mappings":[{"from_button_id":"button.menu_select",
                    "to_button_id":"mod.control:0","mapping_type":"pressed"}],
                "controls":[{"normal":state([0,0,1,1])},{"hover":state([0,1,0,1])},
                    {"pressed":state([1,0,0,1])}]
            }}]
        }})
        .to_string(),
        bindings: Default::default(),
    });
    let mut p = mini_engine_presentation();
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut p, [1280, 720]);
    let at = point(&p, "mod.control:0", 0.5);
    let pixel = |p: &mut UiPresentationRuntime| {
        let rendered = frame(p, [1280, 720]);
        snapshot::rasterize(&rendered)
            .get_pixel(at[0] as u32, at[1] as u32)
            .0
    };
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    assert!(p.mod_ui_mut().mod_panel_events(at, false, false).is_empty());
    assert_eq!(pixel(&mut p), [0, 255, 0, 255]);
    assert_eq!(
        p.mod_ui_mut().mod_panel_events(at, true, true),
        [Event {
            id: "continue".into(),
            value: 1.
        }]
    );
    assert_eq!(pixel(&mut p), [255, 0, 0, 255]);
    assert!(
        p.mod_ui_mut()
            .mod_panel_events([-50.; 2], false, true)
            .is_empty()
    );
    assert_eq!(
        pixel(&mut p),
        [255, 0, 0, 255],
        "press remains captured outside"
    );
    p.mod_ui_mut().mod_panel_events([-50.; 2], false, false);
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut().mod_panel_events(at, false, false);
    assert_eq!(pixel(&mut p), [0, 255, 0, 255]);
    assert_eq!(
        p.mod_ui_mut().mod_panel_events(at, true, false),
        [Event {
            id: "continue".into(),
            value: 1.
        }]
    );
    assert_eq!(pixel(&mut p), [255, 0, 0, 255]);
    assert!(p.mod_ui_mut().mod_panel_events(at, false, false).is_empty());
    assert_eq!(
        pixel(&mut p),
        [0, 255, 0, 255],
        "a quick tap clears its press on the next stationary callback"
    );
    p.mod_ui_mut().cancel_mod_panel_pointer_input();
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut()
        .mod_panel_events([f32::NAN, at[1]], false, true);
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut().set_mod_panel_open(false);
    p.mod_ui_mut().set_mod_panel_open(true);
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
}

#[test]
fn static_local_template_routes_native_click_with_scalar_override() {
    let mut spec = authored();
    let surface = spec.surface.as_mut().unwrap();
    surface.document=json!({"namespace":"extension","route":{"type":"button","size":[70,20],"$action|default":"mod.control:0","button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"$action","mapping_type":"pressed"}]},"screen":{"type":"screen","size":["100%","100%"],"controls":[{"chosen@extension.route":{"offset":[20,20],"$action":"mod.control:0"}}]}}).to_string();
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&spec))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let at = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_events(at, true, true),
        [Event {
            id: "enabled".into(),
            value: 1.0
        }]
    );
}

#[test]
fn static_template_runtime_expression_tracks_bound_visibility_without_rebuilding() {
    let mut spec = authored();
    let surface = spec.surface.as_mut().unwrap();
    surface.document=json!({"namespace":"extension","route":{"type":"button","size":[70,20],"$condition|default":"(#runtime_value = 'On')","bindings":[{"binding_name":"$condition","binding_name_override":"#visible"}],"button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.control:0","mapping_type":"pressed"}]},"screen":{"type":"screen","size":["100%","100%"],"controls":[{"chosen@extension.route":{"offset":[20,20]}}]}}).to_string();
    surface
        .bindings
        .insert("#runtime_value".into(), SurfaceValue::Text("On".into()));
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&spec))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let at = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_events(at, true, true)[0].id,
        "enabled"
    );
    let catalog = Arc::clone(
        presentation
            .mod_ui()
            .mod_panel_state()
            .unwrap()
            .catalog
            .unwrap(),
    );
    spec.surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#runtime_value".into(), SurfaceValue::Text("Off".into()));
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&spec))
        .unwrap();
    frame(&mut presentation, [1280, 720]);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(at, true, true)
            .is_empty()
    );
    assert!(Arc::ptr_eq(
        &catalog,
        presentation
            .mod_ui()
            .mod_panel_state()
            .unwrap()
            .catalog
            .unwrap()
    ));
}

#[test]
fn two_axis_capture_clamps_and_commits_release_without_resuming() {
    let mut spec = panel();
    spec.controls.push(Control::Slider {
        id: "vertical".into(),
        label: "Vertical".into(),
        value: 0.,
        min: 0.,
        max: 100.,
        step: 1.,
    });
    spec.surface = Some(ui::mod_panel::Surface {screen:"vector_test.root".into(),bindings:Default::default(),document:json!({"namespace":"vector_test","root":{"type":"panel","size":[200,150],"controls":[{"area":{"type":"button","size":[100,100],"button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.vector:1:4","mapping_type":"pressed"}]}}]}}).to_string()});
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&spec))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let at = point(&presentation, "mod.vector:1:4", 0.25);
    let events = presentation.mod_ui_mut().mod_panel_events(at, true, true);
    assert_eq!(
        events,
        vec![
            Event {
                id: "strength".into(),
                value: 25.
            },
            Event {
                id: "vertical".into(),
                value: 50.
            }
        ]
    );
    let events = presentation
        .mod_ui_mut()
        .mod_panel_events([20000., 20000.], false, false);
    assert_eq!(
        events,
        vec![
            Event {
                id: "strength".into(),
                value: 100.
            },
            Event {
                id: "vertical".into(),
                value: 100.
            }
        ]
    );
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(at, false, true)
            .is_empty()
    );
}
