use super::super::tests::mini_engine_presentation;
use super::tests::{frame, panel, point};
use super::*;
use serde_json::json;
use ui::mod_panel::{Surface, SurfaceValue};

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
    presentation.set_mod_panel(Some(&authored())).unwrap();
    presentation.set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let toggle = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation.mod_panel_events(toggle, true, true),
        [Event {
            id: "enabled".into(),
            value: 1.0
        }]
    );
    frame(&mut presentation, [1280, 720]);
    let number = point(&presentation, "mod.edit:1", 0.5);
    assert!(presentation.mod_panel_events(number, true, true).is_empty());
    presentation.mod_panel_key("Digit4", Some("42"));
    assert_eq!(
        presentation.mod_panel_key("Enter", None),
        [Event {
            id: "strength".into(),
            value: 42.0
        }]
    );
    frame(&mut presentation, [1280, 720]);
    let choice = point(&presentation, "mod.control:2", 0.5);
    assert!(presentation.mod_panel_events(choice, true, true).is_empty());
    frame(&mut presentation, [1280, 720]);
    let option = point(&presentation, "mod.option:1", 0.5);
    assert_eq!(
        presentation.mod_panel_events(option, true, true),
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
    presentation.set_mod_panel(Some(&panel)).unwrap();
    presentation.set_mod_panel_open(true);
    let before = frame(&mut presentation, [1280, 720]);
    let catalog = Arc::clone(
        presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .catalog
            .as_ref()
            .unwrap(),
    );
    panel.surface.as_mut().unwrap().bindings.insert(
        "#surface_color".into(),
        SurfaceValue::Vector(vec![0., 1., 0., 1.]),
    );
    presentation.set_mod_panel(Some(&panel)).unwrap();
    let after = frame(&mut presentation, [1280, 720]);
    let retained = presentation.form_presentation.mod_panel.as_ref().unwrap();
    assert!(Arc::ptr_eq(&catalog, retained.catalog.as_ref().unwrap()));
    assert!(before.vertices != after.vertices);
    let changed = point(&presentation, "mod.control:0", 0.5);
    assert_eq!(
        presentation.mod_panel_events(changed, true, true)[0].id,
        "enabled"
    );
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
    presentation.set_mod_panel(Some(&panel)).unwrap();
    presentation.set_mod_panel_open(true);
    for size in [[854, 480], [1920, 1280]] {
        let rendered = frame(&mut presentation, size);
        let at = point(&presentation, "mod.control:3", 0.5);
        let frame = presentation
            .form_presentation
            .mod_panel
            .as_ref()
            .unwrap()
            .frame
            .as_ref()
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
        let image = super::super::snapshot::rasterize(&rendered);
        assert_eq!(
            image.get_pixel(at[0] as u32, at[1] as u32).0,
            [0, 255, 0, 255]
        );
        assert_eq!(
            presentation.mod_panel_events(at, true, true),
            [Event {
                id: "binding".into(),
                value: 1.0
            }]
        );
        presentation.mod_panel_events(at, false, false);
    }
    let resized = panel.clone();
    panel.reference_size = Some([600.0, 400.0]);
    assert!(!template::same_shape(&resized, &panel));
}
