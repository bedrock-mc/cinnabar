use bedrock_protocol_1_26_51::{packets::ModalFormRequest, wire};
use protocol::{FormKind, ServerFormModel, UiEvent, WorldEvent, into_world_event};

/// Normalizes a server form without involving its presentation.
fn form(json: &str) -> protocol::FormRequestEvent {
    let event = into_world_event(
        protocol::Packet::new(ModalFormRequest {
            form_id: wire::VarUInt(7),
            form_ui_json: json.into(),
        }),
        0,
    )
    .expect("valid form JSON");
    let Some(WorldEvent::Ui(UiEvent::Form(form))) = event else {
        panic!("form request");
    };
    form
}

#[test]
fn form_json_keeps_deep_unused_metadata_without_losing_buttons() {
    for depth in [17, 129, 20_000] {
        for (open, close) in [("[", "]"), ("{\"unused\":", "}")] {
            let metadata = format!("{}null{}", open.repeat(depth), close.repeat(depth));
            let json = format!(
                r#"{{"metadata":{metadata},"type":"form","title":"Menu","buttons":[{{"text":"First","metadata":{metadata}}},{{"text":"Last"}}]}}"#
            );
            let event = form(&json);
            assert_eq!(event.kind, FormKind::Menu);
            assert_eq!(event.title.as_deref(), Some("Menu"));
            assert_eq!(event.json.as_ref(), json);
            let ServerFormModel::TextMenu(menu) = event.model else {
                panic!("deep metadata rejected at depth {depth}");
            };
            assert_eq!(menu.buttons.len(), 2);
            assert_eq!(menu.buttons[0].as_ref(), "First");
            assert_eq!(menu.buttons[1].as_ref(), "Last");
        }
    }
}

#[test]
fn form_json_validates_malformed_deep_metadata_and_duplicate_members() {
    let metadata = format!("{}null{}", "[".repeat(20_000), "]".repeat(20_000));
    let json = format!(
        r#"{{"metadata":{metadata},"metadata":false,"type":"form","buttons":[{{"text":"First","text":"Last"}}]}}"#
    );
    let ServerFormModel::TextMenu(menu) = form(&json).model else {
        panic!("duplicate metadata");
    };
    assert_eq!(menu.buttons[0].as_ref(), "Last");
    for malformed in [
        format!(r#"{{"type":"form","buttons":[],"metadata":{metadata},}}"#),
        format!(r#"{{"type":"form","buttons":[],"metadata":{metadata}}}"#)
            .replacen("null", "null,", 1),
        format!(r#"{{"type":"form","buttons":[],"metadata":{metadata}}}"#).replacen("]", "}", 1),
    ] {
        assert!(
            into_world_event(
                protocol::Packet::new(ModalFormRequest {
                    form_id: wire::VarUInt(7),
                    form_ui_json: malformed.into(),
                }),
                0
            )
            .is_err()
        );
    }
}

#[test]
fn form_json_keeps_structured_text_limits_separate_from_container_parsing() {
    let text = format!(
        "{}{{\"text\":\"End\"}}{}",
        "{\"rawtext\":[".repeat(20_000),
        "]}".repeat(20_000)
    );
    let event = form(&format!(r#"{{"type":"form","title":{text},"buttons":[]}}"#));
    assert!(matches!(
        event.model,
        ServerFormModel::Unsupported(protocol::UnsupportedForm::Limit)
    ));
    let event = form(&format!(
        r#"{{"type":"form","title":{{"rawtext":{text}}},"buttons":[]}}"#
    ));
    assert!(matches!(event.model, ServerFormModel::TextMenu(_)));
}
