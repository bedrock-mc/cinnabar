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
