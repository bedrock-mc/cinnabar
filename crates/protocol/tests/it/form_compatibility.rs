use protocol::{ServerFormModel, UiEvent, WorldEvent, into_world_event};
use serde_json::json;
use valentine::bedrock::version::v1_26_51::ModalFormRequestPacket;

/// Decodes an authored form through the same normalization used by live packets.
fn model(document: serde_json::Value) -> ServerFormModel {
    let Some(WorldEvent::Ui(UiEvent::Form(event))) = into_world_event(
        ModalFormRequestPacket {
            form_id: 7,
            form_uijson: document.to_string(),
        }
        .into(),
        0,
    )
    .unwrap() else {
        panic!("form request");
    };
    event.model
}

#[test]
fn structured_text_is_supported_in_every_form_family() {
    let text = json!({"rawtext":[{"text":"Hello "},{"translate":"gui.submit"}]});
    for document in [
        json!({"type":"form","title":text,"content":text,"buttons":[{"text":text}]}),
        json!({"type":"modal","title":text,"content":text,"button1":text,"button2":text}),
        json!({"type":"custom_form","title":text,"submit":text,"content":[{"type":"label","text":text}]}),
    ] {
        let form = model(document);
        assert!(
            !matches!(form, ServerFormModel::Unsupported(_)),
            "structured form text rejected: {form:?}"
        );
    }
}

#[test]
fn multiselect_is_a_supported_custom_control() {
    let form = model(json!({"type":"custom_form","content":[
        {"type":"multiselect","text":"Choose","options":["A","B","C"],"default":[0,2]}
    ]}));
    assert!(
        matches!(form, ServerFormModel::Custom(_)),
        "multiselect rejected: {form:?}"
    );
}

#[test]
fn menus_use_buttons_before_elements_and_ignore_unused_metadata() {
    for document in [
        json!({"type":"form","buttons":[{"text":"Button","tooltip":"Metadata"}],"elements":[]}),
        json!({"type":"form","buttons":null,"elements":[{"type":"button","text":"Button"}]}),
        json!({"type":"form","buttons":[{"text":"Button","image":{"type":"path","data":"textures/items/apple","extra":true}}]}),
    ] {
        let ServerFormModel::TextMenu(form) = model(document) else {
            panic!("valid menu rejected");
        };
        assert_eq!(form.buttons.len(), 1);
        assert_eq!(form.buttons[0].as_ref(), "Button");
    }
}

#[test]
fn custom_display_fields_accept_structured_text_without_changing_input_defaults() {
    let text = json!({"rawtext":[{"translate":"gui.submit"}]});
    let ServerFormModel::Custom(form) = model(
        json!({"type":"custom_form","title":text,"submit":text,"content":[
            {"type":"header","text":text}, {"type":"label","text":text}, {"type":"divider"},
            {"type":"toggle","text":text,"tooltip":text},
            {"type":"slider","text":text,"tooltip":text,"min":0,"max":10,"default":20,"timeout":250},
            {"type":"step_slider","text":text,"steps":[text],"tooltip":text},
            {"type":"dropdown","text":text,"options":[text],"tooltip":text,"default":9},
            {"type":"input","text":text,"placeholder":text,"default":"{literal}","tooltip":text},
            {"type":"multiselect","text":text,"options":[text],"tooltip":text,"default":[]}
        ]}),
    ) else {
        panic!("custom form with structured display fields");
    };
    assert_eq!(form.elements.len(), 9);
    assert!(matches!(&form.title, protocol::FormText::Raw(_)));
    let protocol::CustomFormElement::Slider {
        timeout, default, ..
    } = &form.elements[4]
    else {
        panic!("slider");
    };
    assert_eq!(timeout.get(), 0.25);
    assert_eq!(default.get(), 20.0);
    let protocol::CustomFormElement::Input { default, .. } = &form.elements[7] else {
        panic!("input");
    };
    assert_eq!(default.as_ref(), "{literal}");
}

#[test]
fn multiselect_response_is_an_index_array_in_the_custom_response_slot() {
    use valentine::bedrock::version::v1_26_51::McpePacketData;
    let packet = protocol::custom_form_submit_response(
        7,
        &[
            protocol::CustomFormValue::Null,
            protocol::CustomFormValue::MultiSelect(vec![2, 0].into()),
            protocol::CustomFormValue::Input("draft".into()),
        ],
    );
    let McpePacketData::ModalFormResponsePacket(packet) = packet.data else {
        panic!("response");
    };
    assert_eq!(
        packet.json_response.as_deref(),
        Some("[null,[2,0],\"draft\"]")
    );
    assert!(packet.form_cancel_reason.is_none());
}

#[test]
fn structured_display_text_stays_bounded_and_literal_json_stays_literal() {
    let literal = r#"{"rawtext":[{"text":"literal JSON"}]}"#;
    let ServerFormModel::TextMenu(form) =
        model(json!({"type":"form","buttons":[{"text":literal}]}))
    else {
        panic!("menu");
    };
    assert!(matches!(&form.buttons[0], protocol::FormText::Literal(_)));
    assert_eq!(form.buttons[0].as_ref(), literal);
    let too_long = "x".repeat(protocol::MAX_UI_TEXT_BYTES + 1);
    assert!(matches!(
        model(json!({"type":"form","buttons":[{"text":{"rawtext":[{"text":too_long}]}}]})),
        ServerFormModel::Unsupported(protocol::UnsupportedForm::Limit)
    ));
}

#[test]
fn captured_dimension_clash_menu_keeps_every_wire_selection() {
    let document =
        serde_json::from_str(include_str!("../../fixtures/dimension_clash_menu.json")).unwrap();
    let ServerFormModel::TextMenu(form) = model(document) else {
        panic!("captured menu");
    };
    assert_eq!(form.buttons.len(), 346);
    assert_eq!(form.buttons[0].as_ref(), "Close");
    assert_eq!(form.buttons[345].as_ref(), "P");
    assert_eq!(form.button_images.len(), form.buttons.len());
}

#[test]
fn custom_form_icons_share_menu_image_shapes_without_adding_response_slots() {
    for kind in ["path", "url"] {
        let ServerFormModel::Custom(form) = model(
            json!({"type":"custom_form","icon":{"type":kind,"data":"source","metadata":true},"content":[]}),
        ) else {
            panic!("custom form icon");
        };
        assert!(matches!(
            (kind, form.icon),
            ("path", Some(protocol::FormButtonImage::Path(_)))
                | ("url", Some(protocol::FormButtonImage::Url(_)))
        ));
        assert!(form.elements.is_empty());
    }
}

#[test]
fn untouched_signed_indexes_are_not_replaced_by_a_different_option() {
    let ServerFormModel::Custom(form) = model(json!({"type":"custom_form","content":[
        {"type":"dropdown","text":"D","options":["A"],"default":-1},
        {"type":"step_slider","text":"S","steps":["A"],"default":-1}
    ]})) else {
        panic!("custom defaults");
    };
    assert!(matches!(
        &form.elements[0],
        protocol::CustomFormElement::Dropdown { default: -1, .. }
    ));
    assert!(matches!(
        &form.elements[1],
        protocol::CustomFormElement::StepSlider { default: -1, .. }
    ));
}

#[test]
fn bounded_custom_forms_keep_controls_and_options_above_256() {
    let count = 300;
    let controls: Vec<_> = (0..count)
        .map(|_| json!({"type":"toggle","text":"Choice"}))
        .collect();
    let ServerFormModel::Custom(form) = model(json!({"type":"custom_form","content":controls}))
    else {
        panic!("bounded custom control list");
    };
    assert_eq!(form.elements.len(), count);
    for kind in ["dropdown", "multiselect", "step_slider"] {
        let list = if kind == "step_slider" {
            "steps"
        } else {
            "options"
        };
        let options: Vec<_> = (0..count).map(|index| format!("Choice {index}")).collect();
        assert!(
            matches!(
                model(json!({"type":"custom_form","content":[{"type":kind,list:options}]})),
                ServerFormModel::Custom(_)
            ),
            "{kind} options must keep their original indexes"
        );
    }
}

#[test]
fn display_fields_use_boolean_and_empty_scalar_fallbacks() {
    for (value, expected) in [
        (serde_json::json!(true), "true"),
        (serde_json::json!(false), "false"),
        (serde_json::json!(12), ""),
        (serde_json::json!([]), ""),
        (serde_json::json!({"other":"unused"}), ""),
        (
            serde_json::json!({"rawtext":[{"text":"Metadata"}],"extra":true}),
            "Metadata",
        ),
    ] {
        let form = model(
            serde_json::json!({"type":"form", "title":value, "content":value, "buttons":[{"text":value}]}),
        );
        let ServerFormModel::TextMenu(menu) = form else {
            panic!("display scalar fell back: {form:?}");
        };
        assert_eq!(menu.title.as_ref(), expected);
        assert_eq!(menu.content.as_ref(), expected);
        assert_eq!(menu.buttons[0].as_ref(), expected);
        let ServerFormModel::Modal(modal) = model(
            json!({"type":"modal","title":value,"content":value,"button1":value,"button2":value}),
        ) else {
            panic!("modal display scalar");
        };
        assert_eq!(modal.title.as_ref(), expected);
        assert_eq!(modal.content.as_ref(), expected);
        assert_eq!(modal.button1.as_ref(), expected);
        assert_eq!(modal.button2.as_ref(), expected);
        let ServerFormModel::Custom(custom) = model(
            json!({"type":"custom_form","title":value,"submit":value,"content":[{"type":"input","text":value,"placeholder":value,"tooltip":value,"default":"Draft"},{"type":"dropdown","text":value,"options":[value]}]}),
        ) else {
            panic!("custom display scalar");
        };
        assert_eq!(custom.title.as_ref(), expected);
        assert_eq!(custom.submit.as_ref().unwrap().as_ref(), expected);
        let protocol::CustomFormElement::Input {
            text,
            placeholder,
            tooltip,
            default,
        } = &custom.elements[0]
        else {
            panic!("input");
        };
        assert_eq!(text.as_ref(), expected);
        assert_eq!(placeholder.as_ref(), expected);
        assert_eq!(tooltip.as_ref().unwrap().as_ref(), expected);
        assert_eq!(default.as_ref(), "Draft");
        let protocol::CustomFormElement::Dropdown { options, .. } = &custom.elements[1] else {
            panic!("dropdown");
        };
        assert_eq!(options[0].as_ref(), expected);
    }
}

#[test]
fn structured_display_metadata_and_component_precedence_are_preserved() {
    let resolver = protocol::RawTextResolver {
        reader_name: "Player",
        translate: &|key| (key == "arguments").then(|| std::sync::Arc::from("%1|%2")),
        score: &|_, _| None,
        selector: &|_| None,
    };
    for (component, expected) in [
        (json!({"text":"Text","extra":true}), "Text"),
        (
            json!({"translate":"hello","text":"unused","score":{},"extra":true}),
            "hello",
        ),
        (
            json!({"text":"Prefix","rawtext":[{}, {"text":null}, {"text":"Tail","extra":true}]}),
            "PrefixTail",
        ),
        (
            json!({"translate":"arguments","with":[1,"First",{},"Last"]}),
            "First|Last",
        ),
        (
            json!({"translate":"arguments","with":{"rawtext":[{"text":"First","extra":true},{"text":"Last"}],"extra":true}}),
            "First|Last",
        ),
    ] {
        let ServerFormModel::TextMenu(menu) =
            model(json!({"type":"form","buttons":[{"text":{"rawtext":[component]}}]}))
        else {
            panic!("text document");
        };
        let text = match &menu.buttons[0] {
            protocol::FormText::Literal(text) => text.to_string(),
            protocol::FormText::Raw(document) => document.resolve(&resolver).text,
        };
        assert_eq!(text, expected);
    }
}
