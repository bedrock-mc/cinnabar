use bedrock_protocol_1_26_51::packets::ModalFormRequest;
use bedrock_protocol_1_26_51::wire;
use protocol::{ServerFormModel, UiEvent, WorldEvent, into_world_event};
use serde_json::json;

/// Decodes an authored form through the same normalization used by live packets.
fn model(document: serde_json::Value) -> ServerFormModel {
    let Some(WorldEvent::Ui(UiEvent::Form(event))) = into_world_event(
        protocol::Packet::new(ModalFormRequest {
            form_id: wire::VarUInt(7),
            form_ui_json: document.to_string().into(),
        }),
        0,
    )
    .unwrap() else {
        panic!("form request");
    };
    event.model
}

#[test]
fn custom_controls_retain_control_locked_in_the_packet_model() {
    use protocol::{CustomFormElement, FormControlLock};
    for (lock, expected) in [
        ("none", FormControlLock::None),
        ("pregame", FormControlLock::Pregame),
        ("ingame", FormControlLock::Ingame),
    ] {
        let ServerFormModel::Custom(form) = model(json!({"type":"custom_form","content":[
            {"type":"toggle","control_locked":lock},
            {"type":"slider","control_locked":lock},
            {"type":"step_slider","steps":["A"],"control_locked":lock},
            {"type":"input","control_locked":lock}
        ]})) else {
            panic!("custom form");
        };
        assert_eq!(form.elements.len(), 4);
        for element in form.elements.iter() {
            let (CustomFormElement::Toggle { control_locked, .. }
            | CustomFormElement::Slider { control_locked, .. }
            | CustomFormElement::StepSlider { control_locked, .. }
            | CustomFormElement::Input { control_locked, .. }) = element
            else {
                panic!("editable control");
            };
            assert_eq!(*control_locked, expected);
        }
    }
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
    use bedrock_protocol_1_26_51::packets;
    let packet = protocol::custom_form_submit_response(
        7,
        &[
            protocol::CustomFormValue::Null,
            protocol::CustomFormValue::MultiSelect(vec![2, 0].into()),
            protocol::CustomFormValue::Input("draft".into()),
        ],
    );
    let packets::Packet::ModalFormResponse(packet) = *packet.data else {
        panic!("response");
    };
    assert_eq!(
        packet.json_response,
        Some("[null,[2,0],\"draft\"]\n".into())
    );
    assert!(packet.form_cancel_reason.is_none());
}

#[test]
fn literal_json_display_strings_stay_literal() {
    let literal = r#"{"rawtext":[{"text":"literal JSON"}]}"#;
    let ServerFormModel::TextMenu(form) =
        model(json!({"type":"form","buttons":[{"text":literal}]}))
    else {
        panic!("menu");
    };
    assert!(matches!(&form.buttons[0], protocol::FormText::Literal(_)));
    assert_eq!(form.buttons[0].as_ref(), literal);
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
            ..
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

#[test]
fn form_display_strings_keep_long_utf8_in_every_family() {
    let long = "世界🙂".repeat(protocol::MAX_UI_TEXT_BYTES / "世界🙂".len() + 1);
    for value in [json!(long), json!({"rawtext":[{"text":long}]})] {
        let ServerFormModel::TextMenu(menu) =
            model(json!({"type":"form","title":value,"content":value,"buttons":[{"text":value}]}))
        else {
            panic!("long menu text");
        };
        for text in [&menu.title, &menu.content, &menu.buttons[0]] {
            assert_eq!(text.as_ref(), long);
        }
        let ServerFormModel::Modal(modal) = model(
            json!({"type":"modal","title":value,"content":value,"button1":value,"button2":value}),
        ) else {
            panic!("long modal text");
        };
        for text in [&modal.title, &modal.content, &modal.button1, &modal.button2] {
            assert_eq!(text.as_ref(), long);
        }
        let ServerFormModel::Custom(custom) = model(
            json!({"type":"custom_form","title":value,"submit":value,"content":[
                {"type":"label","text":value}, {"type":"header","text":value},
                {"type":"toggle","text":value,"tooltip":value},
                {"type":"slider","text":value,"tooltip":value},
                {"type":"step_slider","text":value,"steps":[value],"tooltip":value},
                {"type":"dropdown","text":value,"options":[value],"tooltip":value},
                {"type":"multiselect","text":value,"options":[value],"tooltip":value},
                {"type":"input","text":value,"placeholder":value,"tooltip":value,"default":"Draft"}
            ]}),
        ) else {
            panic!("long custom text");
        };
        assert_eq!(custom.title.as_ref(), long);
        assert_eq!(custom.submit.as_ref().unwrap().as_ref(), long);
        use protocol::CustomFormElement::*;
        for element in custom.elements.iter() {
            let (text, tooltip) = match element {
                Label { text } | Header { text } => (text, None),
                Toggle { text, tooltip, .. } | Slider { text, tooltip, .. } => {
                    (text, tooltip.as_ref())
                }
                StepSlider {
                    text,
                    steps,
                    tooltip,
                    ..
                } => {
                    assert_eq!(steps[0].as_ref(), long);
                    (text, tooltip.as_ref())
                }
                Dropdown {
                    text,
                    options,
                    tooltip,
                    ..
                }
                | MultiSelect {
                    text,
                    options,
                    tooltip,
                    ..
                } => {
                    assert_eq!(options[0].as_ref(), long);
                    (text, tooltip.as_ref())
                }
                Input {
                    text,
                    placeholder,
                    default,
                    tooltip,
                    ..
                } => {
                    assert_eq!(placeholder.as_ref(), long);
                    assert_eq!(default.as_ref(), "Draft");
                    (text, tooltip.as_ref())
                }
                Divider => panic!("unexpected divider"),
            };
            assert_eq!(text.as_ref(), long);
            if let Some(tooltip) = tooltip {
                assert_eq!(tooltip.as_ref(), long);
            }
        }
    }
}

#[test]
fn form_display_resolution_keeps_expanded_translations_without_changing_chat() {
    let expanded: std::sync::Arc<str> = "Translated 🌍 ".repeat(3_000).into();
    let translate = |key: &str| match key {
        "long" => Some(std::sync::Arc::clone(&expanded)),
        "repeat" => Some(std::sync::Arc::from("%1/%1")),
        _ => None,
    };
    let resolver = protocol::RawTextResolver {
        reader_name: "Player",
        translate: &translate,
        score: &|_, _| None,
        selector: &|_| None,
    };
    for (component, expected) in [
        (json!({"translate":"long"}), expanded.to_string()),
        (
            json!({"translate":"repeat","with":["%long"]}),
            format!("{expanded}/{expanded}"),
        ),
    ] {
        let text = json!({"rawtext":[component]});
        let ServerFormModel::TextMenu(menu) =
            model(json!({"type":"form","buttons":[{"text":text}]}))
        else {
            panic!("translated form");
        };
        let protocol::FormText::Raw(document) = &menu.buttons[0] else {
            panic!("structured form text");
        };
        let resolved = document.resolve(&resolver);
        assert_eq!(resolved.text, expected);
        assert!(!resolved.truncated);
        let chat = protocol::parse_raw_text(&text.to_string())
            .unwrap()
            .resolve(&resolver);
        assert!(chat.text.len() <= protocol::MAX_RAW_TEXT_OUTPUT_BYTES);
        assert!(chat.truncated);
    }
    assert!(
        protocol::parse_raw_text(
            &json!({"rawtext":[{"text":"x".repeat(protocol::MAX_UI_TEXT_BYTES + 1)}]}).to_string()
        )
        .is_err()
    );
}

#[test]
fn form_display_expansion_stops_nested_amplification_with_bounded_allocations() {
    use std::sync::Arc;
    // The safe reduction fails first, before an unbounded resolver can attempt the 100 GB case.
    for (repeats, seed_bytes) in [(10, 2_048), (1_000, 100)] {
        let seed = "x".repeat(seed_bytes);
        let mut component = json!({"text":seed});
        for _ in 0..3 {
            component = json!({"translate":"repeat","with":{"rawtext":[component]}});
        }
        let ServerFormModel::TextMenu(menu) = model(json!({"type":"form","buttons":[
            {"text":{"rawtext":[component, {"selector":"@s"}]}}
        ]})) else {
            panic!("amplified form");
        };
        let protocol::FormText::Raw(document) = &menu.buttons[0] else {
            panic!("rawtext");
        };
        let template: Arc<str> = "%1".repeat(repeats).into();
        let lookups = std::cell::Cell::new(0);
        let translate = |_: &str| {
            lookups.set(lookups.get() + 1);
            Some(Arc::clone(&template))
        };
        let resolver = protocol::RawTextResolver {
            reader_name: "Player",
            translate: &translate,
            score: &|_, _| None,
            selector: &|_| panic!("expansion must stop after exhaustion"),
        };
        let (resolved, allocations) = test_alloc::measure(|| document.resolve(&resolver));
        assert_eq!(resolved.text.len(), protocol::MAX_RAW_TEXT_EXPANSION_BYTES);
        assert!(resolved.text.bytes().all(|byte| byte == b'x'));
        assert!(resolved.truncated);
        assert!(resolved.text.capacity() <= protocol::MAX_RAW_TEXT_EXPANSION_BYTES);
        assert_eq!(lookups.get(), 3);
        assert!(
            allocations.bytes <= protocol::MAX_RAW_TEXT_EXPANSION_BYTES * 8,
            "{allocations:?}"
        );
    }
}

#[test]
fn form_display_expansion_keeps_the_utf8_prefix_when_translation_or_parameter_exhausts_it() {
    use std::sync::Arc;
    let expected = "p".repeat(protocol::MAX_RAW_TEXT_EXPANSION_BYTES - 1);
    let expanded: Arc<str> = format!("{expected}🌍Z").into();
    let translate = |key: &str| match key {
        "long" => Some(Arc::clone(&expanded)),
        "argument" => Some(Arc::from("%1")),
        _ => None,
    };
    for component in [
        json!({"translate":"long"}),
        json!({"translate":"argument","with":["%long"]}),
    ] {
        let ServerFormModel::TextMenu(menu) = model(json!({"type":"form","buttons":[
            {"text":{"rawtext":[component,{"text":"Z"}]}}
        ]})) else {
            panic!("translated form");
        };
        let protocol::FormText::Raw(document) = &menu.buttons[0] else {
            panic!("rawtext");
        };
        let resolved = document.resolve(&protocol::RawTextResolver {
            reader_name: "Player",
            translate: &translate,
            score: &|_, _| None,
            selector: &|_| None,
        });
        assert_eq!(resolved.text, expected);
        assert!(resolved.truncated);
        assert!(resolved.text.capacity() <= protocol::MAX_RAW_TEXT_EXPANSION_BYTES);
    }
}
