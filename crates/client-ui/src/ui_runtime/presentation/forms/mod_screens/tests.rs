use super::*;
use crate::ui_runtime::forms::EngineFrame;
use json_ui::{LayoutEnv, ResolvedControl, TextMeasure, TextureMeta, TextureSource, ViewState};
use server_experience::screen::Rect;

struct EmptyArt;
impl TextMeasure for EmptyArt {
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}
impl TextureSource for EmptyArt {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// Lays out an original button with primary and secondary actions.
fn button_frame(name: &str, size: [f64; 2]) -> EngineFrame {
    let props = serde_json::json!({
        "size": size, "anchor_from": "top_left", "anchor_to": "top_left",
        "button_mappings": [
            {"from_button_id":"button.menu_select", "to_button_id":name, "mapping_type":"pressed"},
            {"from_button_id":"button.menu_secondary_select", "to_button_id":format!("{name}.secondary"), "mapping_type":"pressed"}
        ]
    });
    let root = ResolvedControl {
        name: name.into(),
        control_type: Some("button".into()),
        base: None,
        unresolved_base: None,
        properties: props
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<std::collections::BTreeMap<_, _>>()
            .into(),
        children: Vec::new(),
        factory: None,
    };
    let (laid, report) = json_ui::layout_with(
        &root,
        [200.0, 100.0],
        &LayoutEnv {
            text: &EmptyArt,
            textures: &EmptyArt,
        },
        &ViewState::default(),
    );
    EngineFrame {
        identity: None,
        hits: json_ui::hit_regions(&laid).into(),
        report,
        cancel_target: None,
        origin: [0.0; 2],
        scale: 1.0,
        panel: None,
        edit_texts: Vec::new(),
        top: Vec::new(),
    }
}

/// Installs an overlay button beside a view that also has a full-screen dismiss hit.
fn overlapping_screens() -> UiPresentationRuntime {
    let mut presentation = crate::test_support::mini_engine_presentation();
    let files = Arc::new(screen::Files {
        namespace: "input_test".into(),
        templates: Default::default(),
        textures: Vec::new(),
    });
    presentation.set_mod_screens(Some(ModScreensInput {
        id: "input_test",
        files: &files,
        overlay: None,
        view: None,
        focus: None,
        data: &screen::Modal::default(),
    }));
    let screens = presentation.form_presentation.mod_screens.as_mut().unwrap();
    screens.overlay.frame = Some(button_frame("overlay", [20.0, 20.0]));
    screens.view.frame = Some(button_frame("dismiss", [200.0, 100.0]));
    screens.layout = Some(ScreenLayout {
        screen: "inventory".into(),
        size: GuiSize {
            width: 200.0,
            height: 100.0,
            scale: 1.0,
        },
        gui: Rect {
            x: 50.0,
            y: 20.0,
            width: 100.0,
            height: 60.0,
        },
        exclusions: Vec::new(),
        view: Some(Rect {
            x: 50.0,
            y: 20.0,
            width: 100.0,
            height: 60.0,
        }),
    });
    presentation
}

#[test]
fn overlay_primary_and_secondary_clicks_win_over_full_screen_dismiss_hits() {
    for secondary in [false, true] {
        let mut presentation = overlapping_screens();
        let click = |p: &mut UiPresentationRuntime, point, pressed, released| {
            if secondary {
                p.secondary_press_mod_screens(point, pressed, released)
            } else {
                p.press_mod_screens(point, pressed, released)
            }
        };
        let target = if secondary {
            "overlay.secondary"
        } else {
            "overlay"
        };
        assert_eq!(
            click(&mut presentation, Some([5.0, 5.0]), true, false),
            None
        );
        assert_eq!(
            click(&mut presentation, Some([5.0, 5.0]), false, true),
            Some((target.into(), None))
        );
        assert_eq!(
            click(&mut presentation, Some([5.0, 5.0]), true, false),
            None
        );
        assert_eq!(
            click(&mut presentation, Some([180.0, 80.0]), false, true),
            None
        );
        let target = if secondary {
            "dismiss.secondary"
        } else {
            "dismiss"
        };
        assert_eq!(
            click(&mut presentation, Some([180.0, 80.0]), true, false),
            None
        );
        assert_eq!(
            click(&mut presentation, Some([180.0, 80.0]), false, true),
            Some((target.into(), None))
        );
    }
}

#[test]
fn overlay_wheel_scrolls_blank_viewports_with_and_without_an_open_view() {
    for view_open in [false, true] {
        let mut presentation = overlapping_screens();
        let screens = presentation.form_presentation.mod_screens.as_mut().unwrap();
        if !view_open {
            screens.view.frame = None;
            screens.layout.as_mut().unwrap().view = None;
        }
        let frame = screens.overlay.frame.as_mut().unwrap();
        frame.hits = Arc::from([]);
        frame.report.scrolls.insert(
            "rows".into(),
            json_ui::ScrollMetrics {
                content: 100.0,
                viewport: 20.0,
                viewport_rect: Some([0.0, 0.0, 20.0, 20.0]),
                speed: 10.0,
                ..Default::default()
            },
        );
        presentation.hover_mod_screens(Some([5.0, 5.0]));
        assert_eq!(presentation.scroll_mod_screens([5.0, 5.0], 1.0), None);
        let screens = presentation.form_presentation.mod_screens.as_ref().unwrap();
        assert_eq!(screens.overlay.view.scroll.get("rows"), Some(&10.0));
    }
}

#[test]
fn forbidden_overlay_edit_boxes_release_focus_and_reject_focus_requests() {
    let Some(mut presentation) = crate::test_support::engine_presentation() else {
        eprintln!("skipping overlay edit focus test: missing installed UI carrier; make assets");
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player);
    let template = serde_json::json!({"namespace":"focus_test", "overlay": {
        "type":"panel", "size":["100%","100%"], "controls":[{"search": {
            "type":"edit_box", "size":[20,10], "anchor_from":"top_left", "anchor_to":"top_left",
            "text_box_name":"focus_test.search", "max_length":32, "text_control":"display",
            "bindings":[{"binding_name":"#offset"}],
            "button_mappings":[
                {"from_button_id":"button.menu_select","to_button_id":"button.text_edit_box_selected",
                    "handle_select":true,"handle_deselect":false,"mapping_type":"pressed"},
                {"from_button_id":"button.menu_select","to_button_id":"button.text_edit_box_selected",
                    "handle_select":false,"handle_deselect":true,"mapping_type":"global","consume_event":false},
                {"from_button_id":"button.menu_cancel","to_button_id":"button.text_edit_box_deselected",
                    "handle_select":false,"handle_deselect":true,"mapping_type":"global"}],
            "controls":[{"display":{"type":"label","text":"","size":[20,10]}}]}}]}});
    let files = Arc::new(screen::Files {
        namespace: "focus_test".into(),
        templates: [(
            "ui/overlay.json".into(),
            serde_json::to_vec(&template).unwrap(),
        )]
        .into(),
        textures: Vec::new(),
    });
    let overlay = "ui/overlay.json".to_owned();
    let mut data = screen::Modal::default();
    let redraw =
        |presentation: &mut UiPresentationRuntime, data: &screen::Modal, revision: Option<u64>| {
            let focus = revision.map(|revision| (revision, "focus_test.search".to_owned()));
            presentation.set_mod_screens(Some(ModScreensInput {
                id: "focus_test",
                files: &files,
                overlay: Some(&overlay),
                view: None,
                focus: focus.as_ref(),
                data,
            }));
            presentation
                .build(
                    &player,
                    &runtime,
                    0,
                    [1920, 1080],
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
        };
    redraw(&mut presentation, &data, None);
    let gui = presentation.mod_screen_layout().unwrap().gui;
    let allowed = [gui.x - 24.0, gui.y + 4.0];
    assert!(
        allowed[0] >= 0.0,
        "fixture must leave room beside the container"
    );
    data.set_value("#offset".into(), screen::Value::Numbers(allowed.to_vec()));
    redraw(&mut presentation, &data, Some(1));
    assert!(presentation.mod_text_focused());
    assert_eq!(
        presentation
            .edit_mod_screens(None, false, &["kept".into()], false, 0.0)
            .edits,
        [("focus_test.search".into(), "kept".into())]
    );
    data.set_value(
        "#offset".into(),
        screen::Value::Numbers(vec![gui.x + 4.0, gui.y + 4.0]),
    );
    redraw(&mut presentation, &data, None);
    assert!(
        presentation
            .form_presentation
            .mod_screens
            .as_ref()
            .unwrap()
            .overlay
            .frame
            .as_ref()
            .unwrap()
            .hits
            .iter()
            .all(|hit| hit.kind != json_ui::HitKind::EditBox),
        "fixture edit box did not enter the forbidden panel"
    );
    assert!(
        !presentation.mod_text_focused(),
        "a clipped edit box retained typing"
    );
    assert!(
        !presentation
            .edit_mod_screens(None, false, &[], true, 0.1)
            .escape_consumed
    );
    redraw(&mut presentation, &data, Some(2));
    assert!(
        !presentation.mod_text_focused(),
        "focus-text selected a forbidden edit box"
    );
    data.set_value("#offset".into(), screen::Value::Numbers(allowed.to_vec()));
    redraw(&mut presentation, &data, Some(3));
    assert!(presentation.mod_text_focused());
    assert_eq!(
        presentation
            .edit_mod_screens(None, false, &["!".into()], false, 0.2)
            .edits,
        [("focus_test.search".into(), "kept!".into())]
    );
}
