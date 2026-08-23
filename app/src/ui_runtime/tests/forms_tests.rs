//! Server-form retention and the modal answer lifecycle witnesses.

use std::sync::Arc;

use protocol::{FormKind, FormKind::*, FormRequestEvent, ModalFormResponseSelection};

use super::*;

fn retained(form_id: u32, kind: FormKind, sequence: u64) -> SequencedUiEvent {
    envelope(
        1,
        sequence,
        UiEvent::Form(FormRequestEvent {
            form_id,
            kind,
            title: Some(Arc::from("Dialog")),
            json: Arc::from(r#"{"type":"form","title":"Dialog"}"#),
        }),
    )
}

#[test]
fn form_events_replace_by_id_and_evict_oldest_at_capacity() {
    let mut runtime = UiRuntime::new(1);
    for id in 1..=(MAX_RETAINED_SERVER_FORMS as u32) {
        runtime.apply(retained(id, Menu, u64::from(id))).unwrap();
    }
    let store = runtime.server_forms();
    assert_eq!(store.entries().count(), MAX_RETAINED_SERVER_FORMS);
    assert_eq!(store.dropped_over_capacity(), 0);
    assert_eq!(store.replaced_by_reissue(), 0);

    // One past capacity drops the oldest dialog, not the new arrival.
    runtime
        .apply(retained(100, Menu, MAX_RETAINED_SERVER_FORMS as u64 + 1))
        .unwrap();
    let store = runtime.server_forms();
    assert_eq!(
        store.entries().next().expect("newest survives").form_id,
        2,
        "form 1 was evicted"
    );
    assert_eq!(store.dropped_over_capacity(), 1);

    // A reissued id replaces its retained dialog exactly once.
    runtime
        .apply(envelope(
            1,
            20,
            UiEvent::Form(FormRequestEvent {
                form_id: 3,
                kind: Custom,
                title: None,
                json: Arc::from(r#"{"type":"custom_form"}"#),
            }),
        ))
        .unwrap();
    let store = runtime.server_forms();
    assert_eq!(store.entries().count(), MAX_RETAINED_SERVER_FORMS);
    assert_eq!(store.replaced_by_reissue(), 1);
    let replaced = store.get(3).expect("replacement retained");
    assert_eq!(replaced.kind, Custom);
    assert_eq!(replaced.title, None);
}

#[test]
fn session_replacement_clears_retained_forms_and_pending_responses() {
    let mut runtime = UiRuntime::new(1);
    runtime.apply(retained(5, Menu, 1)).unwrap();
    runtime
        .respond_to_server_form(5, LocalFormAction::SubmitButton(0))
        .unwrap();

    runtime.begin_session(2);

    assert_eq!(runtime.server_forms().entries().count(), 0);
    assert!(
        !flush_form_response::<crate::runtime::network::PacketSendError>(&mut runtime, |_| panic!(
            "a replaced session must not send its stale form response"
        ))
        .unwrap()
    );
}

#[test]
fn button_submission_builds_the_exact_submit_packet_and_closes_the_form() {
    let mut runtime = UiRuntime::new(1);
    runtime.apply(retained(7, Menu, 1)).unwrap();

    runtime
        .respond_to_server_form(7, LocalFormAction::SubmitButton(2))
        .unwrap();
    assert_eq!(
        runtime.server_forms().get(7),
        None,
        "answered dialogs close"
    );

    let mut sent = None;
    assert!(
        flush_form_response(&mut runtime, |packet| {
            sent = Some(packet);
            Ok::<_, ()>(())
        })
        .unwrap()
    );
    let session = protocol::BedrockSession { shield_item_id: 0 };
    let expected =
        protocol::modal_form_submit_response(7, ModalFormResponseSelection::ButtonIndex(2));
    assert_eq!(
        protocol::encode(&sent.unwrap(), &session).unwrap(),
        protocol::encode(&expected, &session).unwrap()
    );
    assert!(
        !flush_form_response(&mut runtime, |_| Ok::<_, ()>(())).unwrap(),
        "the drained slot stays empty"
    );
}

#[test]
fn dismissal_builds_the_cancel_marker_packet_for_any_family() {
    for kind in [Menu, Modal, Custom] {
        let mut runtime = UiRuntime::new(1);
        runtime.apply(retained(9, kind, 1)).unwrap();
        runtime
            .respond_to_server_form(9, LocalFormAction::Dismiss)
            .unwrap();

        let mut sent = None;
        flush_form_response(&mut runtime, |packet| {
            sent = Some(packet);
            Ok::<_, ()>(())
        })
        .unwrap();
        let session = protocol::BedrockSession { shield_item_id: 0 };
        // id 9, response absent(0), cancel present(1), UserClosed wire value 0.
        assert_eq!(
            protocol::encode(&sent.unwrap(), &session).unwrap().as_ref(),
            &[0xfe, 0x05, 101, 0x09, 0x00, 0x01, 0x00]
        );
    }
}

#[test]
fn unsupported_answers_fail_closed_without_touching_state() {
    let mut runtime = UiRuntime::new(1);
    runtime.apply(retained(4, Custom, 1)).unwrap();
    runtime.apply(retained(5, Menu, 2)).unwrap();

    assert_eq!(
        runtime
            .respond_to_server_form(4, LocalFormAction::CustomElements)
            .unwrap_err(),
        FormRespondError::CustomElementsUnsupported
    );
    assert_eq!(
        runtime
            .respond_to_server_form(4, LocalFormAction::SubmitButton(0))
            .unwrap_err(),
        FormRespondError::ButtonAnswerUnsupportedForKind {
            form_id: 4,
            kind: Custom
        }
    );
    assert_eq!(
        runtime
            .respond_to_server_form(404, LocalFormAction::Dismiss)
            .unwrap_err(),
        FormRespondError::UnknownForm { form_id: 404 }
    );

    // Nothing was answered, closed, or staged.
    assert_eq!(runtime.server_forms().entries().count(), 2);
    assert!(!flush_form_response(&mut runtime, |_| Ok::<_, ()>(())).unwrap());
}

#[test]
fn newer_answers_supersede_older_pending_responses_with_accounting() {
    let mut runtime = UiRuntime::new(1);
    runtime.apply(retained(1, Menu, 1)).unwrap();
    runtime.apply(retained(2, Menu, 2)).unwrap();

    runtime
        .respond_to_server_form(1, LocalFormAction::SubmitButton(0))
        .unwrap();
    runtime
        .respond_to_server_form(2, LocalFormAction::Dismiss)
        .unwrap();
    assert_eq!(runtime.server_forms().superseded_responses(), 1);

    let mut sent = None;
    flush_form_response(&mut runtime, |packet| {
        sent = Some(packet);
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(
        protocol::encode(
            &sent.unwrap(),
            &protocol::BedrockSession { shield_item_id: 0 }
        )
        .unwrap()
        .as_ref()[3],
        2,
        "only the latest answer's form id is on the wire"
    );
}

#[test]
fn transport_backpressure_restores_the_pending_response_for_retry() {
    let mut runtime = UiRuntime::new(1);
    runtime.apply(retained(6, Menu, 1)).unwrap();
    runtime
        .respond_to_server_form(6, LocalFormAction::SubmitButton(1))
        .unwrap();

    let attempts = std::cell::Cell::new(0);
    let restored = flush_form_response(&mut runtime, |_| {
        attempts.set(attempts.get() + 1);
        Err("full")
    });
    assert_eq!(restored, Err("full"));
    assert_eq!(attempts.into_inner(), 1);

    let mut retried = None;
    flush_form_response(&mut runtime, |packet| {
        retried = Some(packet);
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(
        protocol::encode(
            &retried.unwrap(),
            &protocol::BedrockSession { shield_item_id: 0 }
        )
        .unwrap(),
        protocol::encode(
            &protocol::modal_form_submit_response(6, ModalFormResponseSelection::ButtonIndex(1)),
            &protocol::BedrockSession { shield_item_id: 0 }
        )
        .unwrap()
    );
}

#[test]
fn stream_dimension_changes_clear_retained_forms_and_pending_responses() {
    let mut runtime = UiRuntime::new(1);
    runtime.note_stream_dimension(0);
    runtime.apply(retained(8, Menu, 1)).unwrap();

    runtime.note_stream_dimension(0);
    assert_eq!(
        runtime.server_forms().entries().count(),
        1,
        "the same dimension keeps dialogs"
    );

    runtime
        .respond_to_server_form(8, LocalFormAction::Dismiss)
        .unwrap();
    runtime.note_stream_dimension(1);

    assert_eq!(runtime.server_forms().entries().count(), 0);
    assert!(
        !flush_form_response(&mut runtime, |_| Ok::<_, ()>(())).unwrap(),
        "a dimension change also drops the staged response"
    );
}
