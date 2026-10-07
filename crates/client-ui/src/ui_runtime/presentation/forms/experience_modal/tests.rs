use super::*;
use std::collections::BTreeMap;

fn files(templates: &[(&str, &str)]) -> screen::Files {
    screen::Files {
        namespace: "demo".into(),
        templates: templates
            .iter()
            .map(|(path, text)| ((*path).to_owned(), text.as_bytes().to_vec()))
            .collect(),
        textures: Vec::new(),
    }
}

/// The carrier's vanilla catalog, as the engine keeps it below every server pack.
fn vanilla() -> Option<Catalog> {
    let carrier = super::super::pack_harness::carrier()?;
    Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .ok()
}

#[test]
fn templates_resolve_against_vanilla_and_their_own_namespace_only() {
    let Some(vanilla) = vanilla() else {
        return;
    };
    let terminal = r##"{"namespace": "demo",
        "terminal@common.empty_panel": {"controls": [{"row@demo.row": {}}]},
        "row": {"type": "label", "text": "#name"}}"##;
    let catalog = modal_catalog(&vanilla, &files(&[("ui/terminal.json", terminal)])).unwrap();
    assert!(catalog.lookup("demo", "terminal").is_some());
    let foreign = r#"{"namespace": "demo", "terminal@cinnabar_experience.indicator": {}}"#;
    assert!(modal_catalog(&vanilla, &files(&[("ui/terminal.json", foreign)])).is_err());
    let server = r#"{"namespace": "demo", "terminal@my_server_pack.panel": {}}"#;
    assert!(modal_catalog(&vanilla, &files(&[("ui/terminal.json", server)])).is_err());
    let rootless = r#"{"namespace": "demo", "panel": {}}"#;
    assert!(modal_catalog(&vanilla, &files(&[("ui/terminal.json", rootless)])).is_err());
    let mut vanilla_namespace = files(&[(
        "ui/terminal.json",
        r#"{"namespace": "common", "terminal": {}}"#,
    )]);
    vanilla_namespace.namespace = "common".into();
    assert!(modal_catalog(&vanilla, &vanilla_namespace).is_err());
}

/// A 1×1 PNG: signature, IHDR, IDAT and IEND.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[test]
fn bundle_textures_draw_and_nothing_outside_textures_resolves() {
    use json_ui::TextureSource;
    let Some(presentation) = super::super::pack_harness::engine_presentation() else {
        return;
    };
    let engine = presentation.form_presentation.engine.as_deref().unwrap();
    let files = vec![("textures/demo/panel.png".to_owned(), PIXEL.to_vec())];
    let set = engine.textures.confined(&files, 7);
    let atlas = set.lock();
    let view = super::super::textures::Textures {
        assets: engine.assets(),
        set: &set,
        atlas: &atlas,
        images: None,
    };
    assert_eq!(
        view.texture("textures/demo/panel").unwrap().pixels,
        [1.0, 1.0]
    );
    for path in [
        "https://example.com/panel.png",
        "textures/../ui/panel",
        "ui/demo/panel",
        "",
    ] {
        assert!(view.texture(path).is_none(), "{path}");
        assert!(view.missing(path), "{path}");
    }
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

/// A terminal with one vanilla-style edit box, `demo.search`, 100×20 GUI units at the top left,
/// whose label `display` shows its text; it needs nothing from the vanilla pack.
const SEARCH: &str = r#"{"namespace": "demo",
    "terminal": {"type": "panel", "size": ["100%", "100%"], "controls": [
        {"search": {"type": "edit_box", "size": [100, 20],
            "anchor_from": "top_left", "anchor_to": "top_left",
            "text_box_name": "demo.search", "max_length": 10, "text_control": "display",
            "button_mappings": [
                {"from_button_id": "button.menu_select", "to_button_id": "button.text_edit_box_selected",
                 "handle_select": true, "handle_deselect": false, "mapping_type": "pressed"},
                {"from_button_id": "button.menu_select", "to_button_id": "button.text_edit_box_selected",
                 "handle_select": false, "handle_deselect": true, "mapping_type": "global",
                 "consume_event": false},
                {"from_button_id": "button.menu_cancel", "to_button_id": "button.text_edit_box_deselected",
                 "handle_select": false, "handle_deselect": true, "mapping_type": "global"}],
            "controls": [{"display": {"type": "label", "text": "", "size": [100, 20]}}]}}]}}"#;

/// The search terminal drawn open at `size` physical pixels, scale 1, over gameplay.
fn drawn(
    modal: &screen::Modal,
    files: &Arc<screen::Files>,
    size: [u32; 2],
) -> UiPresentationRuntime {
    let mut presentation = super::super::tests::mini_engine_presentation();
    redraw(&mut presentation, modal, files, size);
    presentation
}

fn redraw(
    presentation: &mut UiPresentationRuntime,
    modal: &screen::Modal,
    files: &Arc<screen::Files>,
    size: [u32; 2],
) {
    presentation.set_experience_modal(Some(ExperienceModal {
        bundle: "demo",
        files,
        modal,
    }));
    let player_runtime = player_state::PlayerState::new(1);
    presentation
        .build(
            &player_runtime,
            &UiRuntime::new(1),
            0,
            size,
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
}

fn open_search() -> (screen::Modal, Arc<screen::Files>) {
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    (modal, Arc::new(files(&[("ui/terminal.json", SEARCH)])))
}

/// The drawn modal's size is its root's, in GUI units, with the GUI scale that maps them onto the
/// window; a resize changes it, and a closed modal has none.
#[test]
fn a_drawn_modal_reports_its_gui_size() {
    let (mut modal, files) = open_search();
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let size = presentation.experience_modal_size().expect("drawn");
    assert!(size.valid(), "{size:?}");
    assert!((size.width * size.scale - 1280.0).abs() < 1.0, "{size:?}");
    assert!((size.height * size.scale - 720.0).abs() < 1.0, "{size:?}");
    redraw(&mut presentation, &modal, &files, [1600, 900]);
    let wider = presentation.experience_modal_size().expect("drawn");
    assert!(
        (wider.width * wider.scale - 1600.0).abs() < 1.0,
        "{wider:?}"
    );
    modal.open(None);
    redraw(&mut presentation, &modal, &files, [1600, 900]);
    assert_eq!(presentation.experience_modal_size(), None);
}

/// Pointer and keyboard drive the modal's edit box as vanilla's `text_edit_box`: a press selects
/// it, typing edits it and is reported by its `text_box_name`, Escape first deselects it, and only
/// an Escape with nothing selected is left to close the modal.
#[test]
fn edit_boxes_take_typing_and_escape_deselects_first() {
    let (modal, files) = open_search();
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let scale = presentation.experience_modal_size().unwrap().scale as f32;
    let inside = Some([10.0 * scale, 10.0 * scale]);
    let typed = |text: &str| vec![text.to_owned()];
    let none: Vec<String> = Vec::new();
    let pressed = presentation.edit_experience_modal(inside, true, &none, false, 0.0);
    assert!(pressed.edits.is_empty() && !pressed.escape_consumed);
    let edited = presentation.edit_experience_modal(inside, false, &typed("iron"), false, 0.1);
    assert_eq!(
        edited.edits,
        [("demo.search".to_owned(), "iron".to_owned())]
    );
    let back = presentation.edit_experience_modal(inside, false, &typed("\u{8}"), false, 0.2);
    assert_eq!(back.edits, [("demo.search".to_owned(), "iro".to_owned())]);
    let escaped = presentation.edit_experience_modal(inside, false, &none, true, 0.3);
    assert!(escaped.escape_consumed);
    let again = presentation.edit_experience_modal(inside, false, &none, true, 0.4);
    assert!(!again.escape_consumed);
    // Unselected, typing goes nowhere.
    let ignored = presentation.edit_experience_modal(inside, false, &typed("x"), false, 0.5);
    assert!(ignored.edits.is_empty());
}

/// `set-text` reaches the box once, without being reported back; the user's later typing stands.
#[test]
fn host_text_reaches_the_box_once() {
    let (mut modal, files) = open_search();
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    modal.set_text("demo.search".into(), "gold".into());
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let scale = presentation.experience_modal_size().unwrap().scale as f32;
    let inside = Some([10.0 * scale, 10.0 * scale]);
    let none: Vec<String> = Vec::new();
    let pressed = presentation.edit_experience_modal(inside, true, &none, false, 0.0);
    assert!(pressed.edits.is_empty());
    let typed = vec!["!".to_owned()];
    let edited = presentation.edit_experience_modal(inside, false, &typed, false, 0.1);
    assert_eq!(
        edited.edits,
        [("demo.search".to_owned(), "gold!".to_owned())]
    );
    // A later unrelated change does not set the text again.
    modal.set_value("#title".into(), screen::Value::Text("ME".into()));
    redraw(&mut presentation, &modal, &files, [1280, 720]);
    let more = presentation.edit_experience_modal(inside, false, &typed, false, 0.2);
    assert_eq!(
        more.edits,
        [("demo.search".to_owned(), "gold!!".to_owned())]
    );
}

/// Two 50×20 buttons: `cycle` maps a primary and a secondary press to `demo.cycle`, as vanilla's
/// slot buttons do; `plain` maps only a primary press to `demo.plain`.
const BUTTONS: &str = r#"{"namespace": "demo",
    "terminal": {"type": "panel", "size": ["100%", "100%"], "controls": [
        {"cycle": {"type": "button", "size": [50, 20],
            "anchor_from": "top_left", "anchor_to": "top_left",
            "button_mappings": [
                {"from_button_id": "button.menu_select", "to_button_id": "demo.cycle", "mapping_type": "pressed"},
                {"from_button_id": "button.menu_secondary_select", "to_button_id": "demo.cycle", "mapping_type": "pressed"}]}},
        {"plain": {"type": "button", "size": [50, 20], "offset": [60, 0],
            "anchor_from": "top_left", "anchor_to": "top_left",
            "button_mappings": [
                {"from_button_id": "button.menu_select", "to_button_id": "demo.plain", "mapping_type": "pressed"}]}}]}}"#;

/// A secondary press released over the control it began on fires the action its
/// `button.menu_secondary_select` mapping names; a control without one takes no secondary press.
#[test]
fn secondary_presses_fire_the_secondary_mapping() {
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    let files = Arc::new(files(&[("ui/terminal.json", BUTTONS)]));
    let mut presentation = drawn(&modal, &files, [1280, 720]);
    let scale = presentation.experience_modal_size().unwrap().scale as f32;
    let cycle = Some([10.0 * scale, 10.0 * scale]);
    let plain = Some([70.0 * scale, 10.0 * scale]);
    assert_eq!(
        presentation.secondary_press_experience_modal(cycle, true, false),
        None
    );
    assert_eq!(
        presentation.secondary_press_experience_modal(cycle, false, true),
        Some(("demo.cycle".to_owned(), None))
    );
    presentation.secondary_press_experience_modal(plain, true, false);
    assert_eq!(
        presentation.secondary_press_experience_modal(plain, false, true),
        None
    );
    // Released elsewhere, nothing fires.
    presentation.secondary_press_experience_modal(cycle, true, false);
    assert_eq!(
        presentation.secondary_press_experience_modal(plain, false, true),
        None
    );
}
