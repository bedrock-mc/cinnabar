use super::*;
use std::sync::Arc;

fn event(json: &str) -> FormRequestEvent {
    FormRequestEvent {
        form_id: 1234567,
        kind: protocol::FormKind::Menu,
        title: Some(Arc::from("PRIVATE_TEST_TITLE")),
        json: Arc::from(json),
        model: ServerFormModel::Unsupported(UnsupportedForm::Controls),
    }
}

#[test]
fn default_off_does_not_touch_document_or_claim_budget() {
    assert!(!opted_in(None));
    for value in ["", "0", "true", "yes", "2"] {
        assert!(!opted_in(Some(OsStr::new(value))));
    }
    assert!(opted_in(Some(OsStr::new("1"))));
    let budget = AtomicBool::new(false);
    assert_eq!(inspect(false, &budget, &event("not JSON")), None);
    assert!(!budget.load(Ordering::Acquire));
    assert_eq!(
        inspect(true, &budget, &event("not JSON")),
        Some(Summary::ParseRejected)
    );
}

#[test]
fn shared_process_budget_does_not_remint_for_cloned_or_new_events() {
    let budget = Arc::new(AtomicBool::new(false));
    let first = event(r#"{"type":"form","buttons":[]}"#);
    let cloned = first.clone();
    assert!(inspect(true, &budget, &first).is_some());
    assert_eq!(inspect(true, &budget, &cloned), None);
    assert_eq!(inspect(true, &Arc::clone(&budget), &event("{}")), None);
}

#[test]
fn supported_forms_leave_the_process_probe_for_the_first_rejected_form() {
    let budget = AtomicBool::new(false);
    let mut supported = event("not JSON");
    supported.model = ServerFormModel::TextMenu(protocol::TextMenuForm {
        title: Arc::from(""),
        content: Arc::from(""),
        buttons: Arc::from([]),
        button_images: Arc::from([]),
        omitted_images: 0,
    });
    assert_eq!(inspect(true, &budget, &supported), None);
    assert!(!budget.load(Ordering::Acquire));
    assert!(inspect(true, &budget, &event(r#"{"type":"form","buttons":[]}"#)).is_some());
    assert!(budget.load(Ordering::Acquire));
}

#[test]
fn concurrent_claims_have_exactly_one_observation() {
    let budget = Arc::new(AtomicBool::new(false));
    let results: Vec<_> = (0..8)
        .map(|_| {
            let budget = Arc::clone(&budget);
            std::thread::spawn(move || inspect(true, &budget, &event("{}")))
        })
        .collect();
    assert_eq!(
        results
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .filter(Option::is_some)
            .count(),
        1
    );
}

#[test]
fn element_menu_observation_keeps_structure_without_payload_text() {
    let probe = event(
        r#"{"type":"form","elements":[{"type":"button","text":"PRIVATE_LABEL"},{"type":"button","text":"PRIVATE_IMAGE","image":{"type":"path","data":"PRIVATE_PATH"}}]}"#,
    );
    let Summary::Parsed { node, .. } = inspect(true, &AtomicBool::new(false), &probe).unwrap()
    else {
        panic!("shape");
    };
    assert_eq!(node.buttons_shape.kind, Kind::Missing);
    assert_eq!(node.elements_shape.kind, Kind::Array);
    assert_eq!(node.elements_shape.entries, 2);
    assert_eq!(node.elements[0].type_class, TypeClass::Button);
    assert_eq!(node.elements[0].image.shape.kind, Kind::Missing);
    assert_eq!(node.elements[1].image.type_class, TypeClass::Path);
    assert_eq!(node.other_keys, 0);
    let output = format!("{node:?}");
    for private in ["PRIVATE_LABEL", "PRIVATE_IMAGE", "PRIVATE_PATH"] {
        assert!(!output.contains(private));
    }
}

#[test]
fn structural_summary_does_not_contain_payload_text_names_urls_or_ids() {
    let probe = event(
        r#"{"type":"form","title":{"rawtext":[{"text":"TITLE_SECRET"}]},"content":"BODY_SECRET","UNKNOWN_SECRET_KEY":{"secret":"NEVER_LOG"},"buttons":[{"text":"LABEL_SECRET","type":"button","image":{"type":"url","data":"https://secret.invalid/token"}},{"text":{"rawtext":[{"text":"SECOND_SECRET"}]},"image":null}]}"#,
    );
    let Summary::Parsed { model, kind, node } =
        inspect(true, &AtomicBool::new(false), &probe).unwrap()
    else {
        panic!("shape");
    };
    assert_eq!(model, Model::Controls);
    assert_eq!(kind, protocol::FormKind::Menu);
    assert_eq!(node.type_class, TypeClass::Form);
    assert_eq!(node.title.kind, Kind::Object);
    assert_eq!(node.title.rawtext_kind, Kind::Array);
    assert_eq!(node.title.rawtext_entries, 1);
    assert_eq!(node.buttons_shape.entries, 2);
    assert_eq!(node.buttons[0].type_class, TypeClass::Button);
    assert_eq!(node.buttons[0].image.type_class, TypeClass::Url);
    assert_eq!(node.buttons[1].image.shape.kind, Kind::Null);
    assert_eq!(node.buttons[1].text.rawtext_entries, 1);
    assert_eq!(node.other_keys, 1);
    let output = format!("{model:?} {kind:?} {node:?}");
    for private in [
        "TITLE_SECRET",
        "PRIVATE_TEST_TITLE",
        "BODY_SECRET",
        "UNKNOWN_SECRET_KEY",
        "NEVER_LOG",
        "LABEL_SECRET",
        "secret.invalid",
        "SECOND_SECRET",
        "1234567",
    ] {
        assert!(!output.contains(private));
    }
}

#[test]
fn hostile_documents_obey_packet_depth_and_fixed_summary_bounds() {
    let oversized = "x".repeat(protocol::MAX_FORM_JSON_BYTES + 1);
    assert_eq!(
        inspect(true, &AtomicBool::new(false), &event(&oversized)),
        Some(Summary::BoundsRejected)
    );
    let nested = format!(
        "{}0{}",
        "[".repeat(protocol::MAX_FORM_JSON_DEPTH + 1),
        "]".repeat(protocol::MAX_FORM_JSON_DEPTH + 1)
    );
    assert_eq!(
        inspect(true, &AtomicBool::new(false), &event(&nested)),
        Some(Summary::BoundsRejected)
    );
    let buttons = format!(
        r#"{{"buttons":[{}]}}"#,
        vec![r#"{"text":"x"}"#; 300].join(",")
    );
    let Summary::Parsed { node, .. } =
        inspect(true, &AtomicBool::new(false), &event(&buttons)).unwrap()
    else {
        panic!("shape");
    };
    assert_eq!(node.buttons_shape.entries, protocol::MAX_FORM_BUTTONS + 1);
    assert_eq!(node.buttons.len(), 2);
    let Summary::Parsed { node, .. } = inspect(
        true,
        &AtomicBool::new(false),
        &event(r#"{"type":null,"title":"[[[","buttons":[false]}"#),
    )
    .unwrap() else {
        panic!("shape");
    };
    assert_eq!(node.type_class, TypeClass::NonString);
    assert_eq!(node.buttons[0].shape.kind, Kind::Boolean);
    for invalid in ["{} trailing", "[}", "{", "[]"] {
        assert!(matches!(
            inspect(true, &AtomicBool::new(false), &event(invalid)),
            Some(Summary::ParseRejected | Summary::BoundsRejected)
        ));
    }
}
