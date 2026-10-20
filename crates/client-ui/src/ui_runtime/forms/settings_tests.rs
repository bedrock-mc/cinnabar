//! Settings replies must not take input until the player selects the server tab.

use super::*;

/// Supplies an original, minimal server settings reply through normal admission.
fn settings_reply(store: &mut ServerFormStore, id: u32, session: u64) {
    store.admit(
        FormRequestEvent {
            form_id: id,
            kind: FormKind::ServerSettings,
            title: Some("Server Settings".into()),
            json: "{}".into(),
            model: ServerFormModel::Modal(protocol::ModalDialogForm {
                title: "Server Settings".into(),
                content: "Enable feature?".into(),
                button1: "Yes".into(),
                button2: "No".into(),
            }),
        },
        u64::from(id),
        session,
        false,
    );
}

#[test]
fn settings_reply_waits_for_explicit_selection() {
    let mut runtime = UiRuntime::new(1);
    let store = runtime.server_forms_mut();
    store.flush_settings_request(true, || true);
    settings_reply(store, 7, 1);
    assert!(
        !store.owns_input(),
        "a reply must leave the current settings tab usable"
    );
    assert_eq!(store.queued_busy_count(), 0);

    store.select_settings_form(true);
    let identity = store.active().unwrap().identity;
    assert!(store.settings_form_active());
    store
        .respond(identity, LocalFormAction::SubmitButton(0))
        .unwrap();
    let mut packets = Vec::new();
    flush_form_response(
        &mut runtime,
        |_| false,
        |packet| {
            packets.push(packet);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        packets,
        [modal_form_submit_response(
            7,
            ModalFormResponseSelection::ModalButton(true)
        )]
    );
    runtime.server_forms_mut().select_settings_form(true);
    assert!(
        !runtime.server_forms().owns_input(),
        "a submitted form must not reopen each frame"
    );
}

#[test]
fn leaving_the_tab_keeps_late_replies_in_the_cache() {
    let mut store = ServerFormStore::default();
    store.select_settings_form(true);
    settings_reply(&mut store, 7, 1);
    assert!(store.settings_form_active());
    store.select_settings_form(false);
    assert!(!store.owns_input());
    settings_reply(&mut store, 8, 1);
    assert!(!store.owns_input());
    assert_eq!(store.queued_busy_count(), 0);
    store.select_settings_form(true);
    assert_eq!(store.active().unwrap().form_id, 8);
}

#[test]
fn clearing_the_session_retires_cached_settings_and_request_state() {
    let mut store = ServerFormStore::default();
    store.flush_settings_request(true, || true);
    settings_reply(&mut store, 7, 1);
    store.clear();
    store.select_settings_form(true);
    assert!(!store.owns_input());
    let mut requested = false;
    store.flush_settings_request(true, || {
        requested = true;
        true
    });
    assert!(requested, "the new session must request its own settings");
}

/// Receives a custom settings draft with editable defaults.
fn custom_settings(store: &mut ServerFormStore, id: u32) {
    store.admit(
        FormRequestEvent {
            form_id: id,
            kind: FormKind::ServerSettings,
            title: Some("Settings".into()),
            json: "{}".into(),
            model: ServerFormModel::Custom(protocol::CustomForm {
                icon: None,
                title: "Settings".into(),
                submit: None,
                elements: vec![
                    protocol::CustomFormElement::Toggle {
                        text: "Enabled".into(),
                        control_locked: Default::default(),
                        default: false,
                        tooltip: None,
                    },
                    protocol::CustomFormElement::Input {
                        text: "Name".into(),
                        control_locked: Default::default(),
                        placeholder: "".into(),
                        default: "Initial".into(),
                        tooltip: None,
                    },
                ]
                .into(),
            }),
        },
        u64::from(id),
        1,
        false,
    );
}

/// Drains accepted answers and rejects any duplicate created by a second flush.
fn settings_packets(runtime: &mut UiRuntime) -> Vec<Packet> {
    let mut packets = Vec::new();
    flush_form_response(
        runtime,
        |_| false,
        |packet| {
            packets.push(packet);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        flush_form_response(runtime, |_| false, |_| panic!("duplicate final answer")),
        Ok(false)
    );
    packets
}

/// Makes both settings values visibly different from their defaults.
fn edit_settings(store: &mut ServerFormStore) {
    store.engine_mut().values[0] = FormValue::Toggle(true);
    store.engine_mut().values[1] = FormValue::Text("Edited".into());
}

/// The final answer corresponding to the edited settings draft.
fn edited_settings_answer(id: u32) -> Packet {
    custom_form_submit_response(
        id,
        &[
            CustomFormValue::Toggle(true),
            CustomFormValue::Input("Edited".into()),
        ],
    )
}

#[test]
fn settings_termination_submits_edits_once_and_tab_changes_keep_the_draft() {
    let mut runtime = UiRuntime::new(1);
    let store = runtime.server_forms_mut();
    store.flush_settings_request(true, || true);
    store.select_settings_form(true);
    custom_settings(store, 7);
    edit_settings(store);
    let original = store.active().unwrap().identity;
    store.select_settings_form(false);
    store.flush_settings_request(true, || panic!("request repeated on tab switch"));
    assert!(!store.owns_input());
    assert!(settings_packets(&mut runtime).is_empty());
    let store = runtime.server_forms_mut();
    store.select_settings_form(true);
    assert_eq!(store.active().unwrap().identity, original);
    assert_eq!(store.engine().values[1], FormValue::Text("Edited".into()));
    store.select_settings_form(false);
    store.flush_settings_request(false, || panic!("request while closed"));
    store.flush_settings_request(false, || panic!("request while closed"));
    assert_eq!(settings_packets(&mut runtime), [edited_settings_answer(7)]);
    let store = runtime.server_forms_mut();
    store.flush_settings_request(true, || true);
    store.select_settings_form(true);
    assert!(
        store.active().is_none(),
        "a new visit must wait for a fresh reply"
    );
}

#[test]
fn settings_termination_keeps_each_visit_answer_through_backpressure() {
    let mut runtime = UiRuntime::new(1);
    for id in [7, 8, 7] {
        let store = runtime.server_forms_mut();
        store.flush_settings_request(true, || true);
        store.select_settings_form(true);
        custom_settings(store, id);
        // A later controller can receive its model while an earlier answer is Full.
        if store.settings_form_active() {
            edit_settings(store);
        }
        store.select_settings_form(false);
        store.flush_settings_request(false, || false);
        assert_eq!(
            flush_form_response(&mut runtime, |_| false, |_| Err(FormTransportError::Full)),
            Err(FormTransportError::Full)
        );
    }
    let packets = settings_packets(&mut runtime);
    assert_eq!(packets.len(), 3);
    assert_eq!(packets[0], edited_settings_answer(7));
    assert_eq!(
        packets[1],
        custom_form_submit_response(
            8,
            &[
                CustomFormValue::Toggle(false),
                CustomFormValue::Input("Initial".into())
            ]
        )
    );
    assert_eq!(
        packets[2],
        custom_form_submit_response(
            7,
            &[
                CustomFormValue::Toggle(false),
                CustomFormValue::Input("Initial".into())
            ]
        )
    );
}

#[test]
fn settings_termination_after_explicit_submit_never_answers_twice() {
    let mut runtime = UiRuntime::new(1);
    let store = runtime.server_forms_mut();
    store.flush_settings_request(true, || true);
    store.select_settings_form(true);
    custom_settings(store, 7);
    edit_settings(store);
    store
        .respond(
            store.active().unwrap().identity,
            LocalFormAction::CustomElements,
        )
        .unwrap();
    assert_eq!(settings_packets(&mut runtime), [edited_settings_answer(7)]);
    let store = runtime.server_forms_mut();
    store.select_settings_form(false);
    store.select_settings_form(true);
    assert!(
        store.active().is_none(),
        "submitted settings must stay retired on tab changes"
    );
    store.flush_settings_request(false, || false);
    assert!(settings_packets(&mut runtime).is_empty());
}

#[test]
fn settings_termination_preserves_an_ordinary_answer_and_a_hidden_settings_draft() {
    let mut runtime = UiRuntime::new(1);
    let store = runtime.server_forms_mut();
    store.flush_settings_request(true, || true);
    store.select_settings_form(true);
    custom_settings(store, 7);
    edit_settings(store);
    store.select_settings_form(false);
    store.admit(
        FormRequestEvent {
            form_id: 7,
            kind: FormKind::Modal,
            title: None,
            json: "{}".into(),
            model: ServerFormModel::Modal(protocol::ModalDialogForm {
                title: "Menu".into(),
                content: "Choose".into(),
                button1: "Yes".into(),
                button2: "No".into(),
            }),
        },
        8,
        1,
        false,
    );
    store
        .respond(
            store.active().unwrap().identity,
            LocalFormAction::SubmitButton(0),
        )
        .unwrap();
    store.flush_settings_request(false, || false);
    assert_eq!(
        settings_packets(&mut runtime),
        [
            modal_form_submit_response(7, ModalFormResponseSelection::ModalButton(true)),
            edited_settings_answer(7),
        ]
    );
}

#[test]
fn dismissing_custom_settings_submits_its_values_before_controller_termination() {
    let mut runtime = UiRuntime::new(1);
    let store = runtime.server_forms_mut();
    store.flush_settings_request(true, || true);
    store.select_settings_form(true);
    custom_settings(store, 7);
    edit_settings(store);
    store
        .respond(store.active().unwrap().identity, LocalFormAction::Dismiss)
        .unwrap();
    store.flush_settings_request(false, || false);
    assert_eq!(settings_packets(&mut runtime), [edited_settings_answer(7)]);
}
