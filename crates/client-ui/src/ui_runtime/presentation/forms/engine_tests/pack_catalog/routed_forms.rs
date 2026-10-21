use json_ui::{CustomElement, CustomForm, FormModel, ResolvedControl};
use std::path::Path;

/// Reads an explicitly supplied original UI pack for a rendered form contract test.
fn files(root: &Path, directory: &Path, result: &mut Vec<(String, Vec<u8>)>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(root, &path, result);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            result.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                std::fs::read(path).unwrap(),
            ));
        }
    }
}

/// Retains a short resolved-tree diagnostic when a pack fails to create its fields.
fn describe(node: &ResolvedControl, out: &mut Vec<String>) {
    if out.len() >= 180 {
        return;
    }
    out.push(format!(
        "{} type={:?} visible={:?} text={:?} length={:?} children={} factory={:?}",
        node.name,
        node.control_type,
        node.properties.get("visible"),
        node.properties.get("text"),
        node.properties
            .get("property_bag")
            .and_then(|bag| bag.get("#collection_length")),
        node.children.len(),
        node.factory.as_ref().map(|factory| &factory.control_ids)
    ));
    if node.properties.get("visible") == Some(&serde_json::json!(false)) {
        return;
    }
    for child in &node.children {
        describe(child, out);
    }
}

#[test]
fn routed_custom_form_retains_its_label_and_toggle() {
    let Some(root) = std::env::var_os("CINNABAR_TEST_FORM_UI_DIR") else {
        eprintln!(
            "skipping routed_custom_form_retains_its_label_and_toggle: missing CINNABAR_TEST_FORM_UI_DIR fixture"
        );
        return;
    };
    let Some(base) = super::vanilla("routed_custom_form_retains_its_label_and_toggle") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let mut layer = Vec::new();
    files(&root, &root.join("ui"), &mut layer);
    let catalog = super::layer_pack_catalog(&base, &[layer]);
    let model = FormModel::Custom(CustomForm {
        title: "Form fixture".into(),
        submit_text: "Submit".into(),
        submit_visible: true,
        elements: vec![
            CustomElement::Label {
                text: "A visible label".into(),
            },
            CustomElement::Toggle {
                enabled: true,
                text: "Enabled".into(),
                on: false,
                tooltip: String::new(),
            },
        ],
        ..Default::default()
    });
    let context = ui_screens::menu_screens::retail_context();
    let tree = json_ui::bind_form(&model, &catalog, &context).unwrap();
    let mut descriptions = Vec::new();
    describe(&tree, &mut descriptions);
    assert!(
        descriptions
            .iter()
            .any(|line| line.contains("A visible label")),
        "fields were not created:\n{}",
        descriptions.join("\n")
    );
}

#[test]
fn admitted_settings_forms_keep_every_interactive_element() {
    let (Some(root), Some(fixtures)) = (
        std::env::var_os("CINNABAR_TEST_FORM_UI_DIR"),
        std::env::var_os("CINNABAR_TEST_FORM_FIXTURES_DIR"),
    ) else {
        eprintln!(
            "skipping admitted_settings_forms_keep_every_interactive_element: missing original pack/form fixtures"
        );
        return;
    };
    let Some(mut presentation) =
        crate::test_support::engine_presentation_with(super::super::super::pack_harness::font())
    else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let mut layer = Vec::new();
    files(&root, &root.join("ui"), &mut layer);
    let pack = super::super::super::ServerUiPack {
        ui_layers: vec![layer],
        ..Default::default()
    };
    let mut paths: Vec<_> = std::fs::read_dir(fixtures)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "the form fixture directory is empty");
    for path in paths {
        let raw = std::fs::read_to_string(&path).unwrap();
        let packet = protocol::test_support::packets::modal_form_request_packet(1, &raw);
        let Some(protocol::WorldEvent::Ui(event)) = protocol::into_world_event(packet, 0).unwrap()
        else {
            panic!("form was not decoded: {}", path.display());
        };
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = crate::ui_runtime::UiRuntime::new(1);
        runtime.set_server_ui(Some(std::sync::Arc::new(pack.clone())));
        runtime
            .apply(
                &mut player,
                crate::ui_runtime::SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 1,
                    local_millis: 0,
                    server_tick: None,
                    event,
                },
            )
            .unwrap();
        presentation
            .build(
                &player,
                &runtime,
                100,
                [1280, 65_536],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let entry = runtime.server_forms().active().unwrap();
        let frame = presentation
            .form_engine_frame(entry.identity)
            .unwrap_or_else(|| panic!("no engine form: {}", path.display()));
        if let protocol::ServerFormModel::Custom(custom) = &entry.model {
            for (index, element) in custom.elements.iter().enumerate() {
                if matches!(
                    element,
                    protocol::CustomFormElement::Label { .. }
                        | protocol::CustomFormElement::Header { .. }
                        | protocol::CustomFormElement::Divider
                ) {
                    continue;
                }
                assert!(
                    frame
                        .hits
                        .iter()
                        .any(|hit| hit.collection_index == Some(index)
                            && matches!(
                                hit.kind,
                                json_ui::HitKind::Button
                                    | json_ui::HitKind::Toggle
                                    | json_ui::HitKind::Dropdown
                                    | json_ui::HitKind::Slider
                                    | json_ui::HitKind::EditBox
                            )),
                    "{} element {index} {element:?} has no interactive control; hits={:?}",
                    path.display(),
                    frame
                        .hits
                        .iter()
                        .map(|hit| (&hit.kind, hit.collection_index, &hit.key))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}
