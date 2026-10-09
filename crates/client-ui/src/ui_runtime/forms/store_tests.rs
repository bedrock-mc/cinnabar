//! Modal, element-menu, and custom-form answers leave as their vanilla wire shapes.
use super::{FormValue, LocalFormAction, flush_form_response};
use crate::ui_runtime::UiRuntime;
use protocol::{
    CustomForm, CustomFormElement, CustomFormValue, ElementMenuForm, FormKind, FormRequestEvent,
    MenuElement, ModalDialogForm, ModalFormResponseSelection, Packet, ServerFormModel,
    custom_form_submit_response, modal_form_submit_response,
};
use std::sync::Arc;

fn admit(model: ServerFormModel, kind: FormKind) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    let session = runtime.session_id();
    runtime.server_forms_mut().admit(
        FormRequestEvent {
            form_id: 4,
            kind,
            title: None,
            json: Arc::from("{}"),
            model,
        },
        1,
        session,
        false,
    );
    runtime
}

fn sent(runtime: &mut UiRuntime) -> Vec<Packet> {
    let mut packets = Vec::new();
    while flush_form_response(runtime, |packet| {
        packets.push(packet);
        Ok(())
    })
    .unwrap()
    {}
    packets
}

fn answer(runtime: &mut UiRuntime, action: LocalFormAction) {
    let identity = runtime.server_forms().active().unwrap().identity;
    runtime.respond_to_server_form(identity, action).unwrap();
}

#[test]
fn modal_buttons_answer_true_then_false() {
    for (button, choice) in [(0, true), (1, false)] {
        let mut runtime = admit(
            ServerFormModel::Modal(ModalDialogForm {
                title: protocol::FormText::from("T"),
                content: protocol::FormText::from("C"),
                button1: protocol::FormText::from("Yes"),
                button2: protocol::FormText::from("No"),
            }),
            FormKind::Modal,
        );
        answer(&mut runtime, LocalFormAction::SubmitButton(button));
        assert_eq!(
            sent(&mut runtime),
            vec![modal_form_submit_response(
                4,
                ModalFormResponseSelection::ModalButton(choice)
            )]
        );
    }
}

#[test]
fn element_menu_indexes_count_buttons_only() {
    let mut runtime = admit(
        ServerFormModel::ElementMenu(ElementMenuForm {
            title: protocol::FormText::from("T"),
            content: protocol::FormText::from("C"),
            elements: vec![
                MenuElement::Label(protocol::FormText::from("L")),
                MenuElement::Button {
                    text: protocol::FormText::from("A"),
                    image: None,
                },
            ]
            .into(),
        }),
        FormKind::Menu,
    );
    let identity = runtime.server_forms().active().unwrap().identity;
    assert!(
        runtime
            .respond_to_server_form(identity, LocalFormAction::SubmitButton(1))
            .is_err(),
        "only one button exists"
    );
    answer(&mut runtime, LocalFormAction::SubmitButton(0));
    assert_eq!(
        sent(&mut runtime),
        vec![modal_form_submit_response(
            4,
            ModalFormResponseSelection::ButtonIndex(0)
        )]
    );
}

#[test]
fn custom_form_submits_the_edited_values_in_order() {
    let mut runtime = admit(
        ServerFormModel::Custom(CustomForm {
            icon: None,
            title: protocol::FormText::from("T"),
            elements: vec![
                CustomFormElement::Label {
                    text: protocol::FormText::from("L"),
                },
                CustomFormElement::Toggle {
                    text: protocol::FormText::from("On"),
                    default: false,
                    tooltip: None,
                },
                CustomFormElement::Input {
                    text: protocol::FormText::from("Name"),
                    placeholder: protocol::FormText::from(""),
                    default: Arc::from("Ste"),
                    tooltip: None,
                },
            ]
            .into(),
            submit: None,
        }),
        FormKind::Custom,
    );
    let engine = runtime.server_forms_mut().engine_mut();
    assert_eq!(engine.values[2], FormValue::Text("Ste".into()));
    engine.values[1] = FormValue::Toggle(true);
    engine.values[2] = FormValue::Text("Steve".into());
    answer(&mut runtime, LocalFormAction::CustomElements);
    assert_eq!(
        sent(&mut runtime),
        vec![custom_form_submit_response(
            4,
            &[
                CustomFormValue::Null,
                CustomFormValue::Toggle(true),
                CustomFormValue::Input("Steve".into())
            ]
        )]
    );
    assert!(
        runtime.server_forms().engine().values.is_empty(),
        "answering clears the live values"
    );
}
