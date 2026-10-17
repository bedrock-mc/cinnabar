use std::collections::BTreeMap;

use json_ui::{
    CollectionItem, DataSource, LayoutEnv, ResolvedControl, Scalar, TextMeasure, TextureMeta,
    TextureSource, ViewState,
};
use server_experience::screen;

use super::{EngineFrame, MAX_UI_VALUES, TemplateScreen, data_source};

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

/// Lays out an original edit box without runtime art or a renderer.
fn edit_frame(control: &str) -> EngineFrame {
    edit_frame_with(control, serde_json::json!([]))
}

/// An edit box with vanilla `text_edit_box`'s select and deselect mappings.
fn selectable_edit_frame(control: &str) -> EngineFrame {
    edit_frame_with(
        control,
        serde_json::json!([
            { "from_button_id": "button.menu_select", "to_button_id": "button.text_edit_box_selected", "handle_select": true, "handle_deselect": false, "mapping_type": "pressed" },
            { "from_button_id": "button.menu_select", "to_button_id": "button.text_edit_box_selected", "handle_select": false, "handle_deselect": true, "mapping_type": "global", "consume_event": false }
        ]),
    )
}

fn edit_frame_with(control: &str, mappings: serde_json::Value) -> EngineFrame {
    let root = ResolvedControl {
        name: "edit".into(),
        control_type: Some("edit_box".into()),
        base: None,
        unresolved_base: None,
        properties: BTreeMap::from([
            ("size".into(), serde_json::json!([100, 20])),
            ("text_box_name".into(), serde_json::json!(control)),
            (
                "max_length".into(),
                serde_json::json!(server_experience::policy::MAX_EDIT_TEXT_BYTES),
            ),
            ("button_mappings".into(), mappings),
            ("text_control".into(), serde_json::json!("display")),
        ])
        .into(),
        children: vec![ResolvedControl {
            name: "display".into(),
            control_type: Some("label".into()),
            base: None,
            unresolved_base: None,
            properties: BTreeMap::from([
                ("text".into(), serde_json::json!("")),
                ("size".into(), serde_json::json!([100, 20])),
            ])
            .into(),
            children: Vec::new(),
            factory: None,
        }],
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

#[test]
fn closed_template_keeps_only_latest_text_and_applies_it_once_when_opened() {
    let mut screen = TemplateScreen::default();
    let mut modal = screen::Modal::default();
    for revision in 0..MAX_UI_VALUES * 4 {
        modal.set_text("query".into(), revision.to_string());
        modal.check().unwrap();
        assert!(screen.follow(None, &modal));
        screen.apply_pending_texts();
        assert_eq!(screen.pending_texts.len(), 1);
        assert!(screen.frame.is_none());
        assert!(screen.view.components.is_empty());
    }
    modal.set_text("other".into(), "other value".into());
    modal.set_text("query".into(), "latest".into());
    screen.follow(None, &modal);
    assert_eq!(
        screen.pending_texts,
        [
            ("other".to_owned(), "other value".to_owned()),
            ("query".to_owned(), "latest".to_owned())
        ]
    );

    let template = "ui/view.json".to_owned();
    screen.follow(Some(&template), &modal);
    screen.frame = Some(edit_frame("query"));
    let edit_key = screen.frame.as_ref().unwrap().hits[0].key.clone();
    screen.apply_pending_texts();
    assert!(screen.pending_texts.is_empty());
    assert_eq!(
        screen.view.components.edit(&edit_key).unwrap().text,
        "latest"
    );

    let hits = screen.frame.as_ref().unwrap().hits.clone();
    screen
        .dispatcher
        .set_edit_text(&hits, &mut screen.view, "query", "later edit");
    modal.set_value("#unrelated".into(), screen::Value::Bool(true));
    screen.follow(Some(&template), &modal);
    screen.apply_pending_texts();
    assert_eq!(
        screen.view.components.edit(&edit_key).unwrap().text,
        "later edit"
    );
}

#[test]
fn pending_texts_follow_the_current_modal_control_budget() {
    let mut screen = TemplateScreen::default();
    let mut modal = screen::Modal::default();
    for value in ["first", "replacement"] {
        for index in 0..MAX_UI_VALUES {
            modal.set_text(format!("box_{index}"), value.into());
            modal.check().unwrap();
            screen.follow(None, &modal);
            assert!(screen.pending_texts.len() <= MAX_UI_VALUES);
        }
    }
    assert_eq!(screen.pending_texts.len(), MAX_UI_VALUES);
    assert!(
        screen
            .pending_texts
            .iter()
            .all(|(_, text)| text == "replacement")
    );

    modal.texts.clear();
    modal.set_text("new_box".into(), "new value".into());
    modal.check().unwrap();
    screen.follow(None, &modal);
    assert_eq!(
        screen.pending_texts,
        [("new_box".to_owned(), "new value".to_owned())]
    );
}

#[test]
fn bound_values_and_rows_reach_the_engine_bindings() {
    let mut modal = screen::Modal::default();
    modal.set_value("#title".into(), screen::Value::Text("§eME".into()));
    modal.set_value(
        "#color".into(),
        screen::Value::Numbers(vec![1.0, 0.5, 0.0, 1.0]),
    );
    modal.set_collection(
        "items".into(),
        vec![BTreeMap::from([
            ("#count".to_owned(), screen::Value::Integer(64)),
            ("#shown".to_owned(), screen::Value::Bool(false)),
        ])],
    );
    let mut expected = DataSource::new();
    expected.set_global("#title", Scalar::Text("§eME".into()));
    expected.set_global(
        "#color",
        Scalar::Json(serde_json::json!([1.0, 0.5, 0.0, 1.0])),
    );
    expected.set_collection(
        "items",
        vec![
            CollectionItem::default()
                .with("#count", Scalar::Int(64))
                .with("#shown", Scalar::Bool(false)),
        ],
    );
    assert_eq!(data_source(&modal), expected);
}

#[test]
fn a_deselected_box_selects_again_on_the_next_press() {
    let template = "ui/overlay.json".to_owned();
    let mut screen = TemplateScreen::default();
    let mut modal = screen::Modal::default();
    screen.follow(Some(&template), &modal);
    screen.frame = Some(selectable_edit_frame("query"));
    let on_box = Some([60.0, 50.0]);
    let typed = |text: &str| vec![text.to_owned()];

    screen.edit(on_box, true, &[], false, 0.0);
    assert!(screen.text_focused());
    let first = screen.edit(on_box, false, &typed("iron"), false, 0.0);
    assert_eq!(first.edits, [("query".to_owned(), "iron".to_owned())]);
    // Pressing outside deselects, then the host clears the box (a secondary press on it).
    screen.edit(Some([5.0, 5.0]), true, &[], false, 1.0);
    assert!(!screen.text_focused());
    modal.set_text("query".into(), String::new());
    screen.follow(Some(&template), &modal);
    screen.apply_pending_texts();

    let second = screen.edit(on_box, true, &typed("stone"), false, 2.0);
    assert!(screen.text_focused());
    assert_eq!(second.edits, [("query".to_owned(), "stone".to_owned())]);
}
