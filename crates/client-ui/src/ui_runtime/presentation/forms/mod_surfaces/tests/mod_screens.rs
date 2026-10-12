use std::sync::Arc;

use mod_ui::mod_screens::ModScreensInput;
use server_experience::screen;

use crate::ui_runtime::presentation::UiPresentationRuntime;

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
    let gui = presentation.mod_ui().mod_screen_layout().unwrap().gui;
    let allowed = [gui.x - 24.0, gui.y + 4.0];
    assert!(
        allowed[0] >= 0.0,
        "fixture must leave room beside the container"
    );
    data.set_value("#offset".into(), screen::Value::Numbers(allowed.to_vec()));
    redraw(&mut presentation, &data, Some(1));
    assert!(presentation.mod_ui().mod_text_focused());
    assert_eq!(
        presentation
            .mod_ui_mut()
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
            .mod_ui()
            .mod_screens_overlay_frame()
            .unwrap()
            .hits
            .iter()
            .all(|hit| hit.kind != json_ui::HitKind::EditBox),
        "fixture edit box did not enter the forbidden panel"
    );
    assert!(
        !presentation.mod_ui().mod_text_focused(),
        "a clipped edit box retained typing"
    );
    assert!(
        !presentation
            .mod_ui_mut()
            .edit_mod_screens(None, false, &[], true, 0.1)
            .escape_consumed
    );
    redraw(&mut presentation, &data, Some(2));
    assert!(
        !presentation.mod_ui().mod_text_focused(),
        "focus-text selected a forbidden edit box"
    );
    data.set_value("#offset".into(), screen::Value::Numbers(allowed.to_vec()));
    redraw(&mut presentation, &data, Some(3));
    assert!(presentation.mod_ui().mod_text_focused());
    assert_eq!(
        presentation
            .mod_ui_mut()
            .edit_mod_screens(None, false, &["!".into()], false, 0.2)
            .edits,
        [("focus_test.search".into(), "kept!".into())]
    );
}
