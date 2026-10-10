//! Form authority, overlap policy and definite-unsent backpressure witnesses.
use super::*;
use protocol::{FormKind, FormRequestEvent, ServerFormModel, TextMenuForm, UnsupportedForm};
use std::sync::Arc;

pub(super) fn retained(form_id: u32, sequence: u64) -> SequencedUiEvent {
    envelope(
        1,
        sequence,
        UiEvent::Form(FormRequestEvent {
            form_id,
            kind: FormKind::Menu,
            title: Some(Arc::from("Choose 世界")),
            json: Arc::from("{}"),
            model: ServerFormModel::TextMenu(TextMenuForm {
                title: protocol::FormText::from("Choose 世界"),
                content: protocol::FormText::from("Pick one"),
                buttons: vec![
                    protocol::FormText::from("First ✓"),
                    protocol::FormText::from("第二"),
                ]
                .into(),
                button_images: [].into(),
                omitted_images: 0,
            }),
        }),
    )
}
fn identity(runtime: &UiRuntime) -> ServerFormIdentity {
    runtime.server_forms().active().unwrap().identity
}
fn bytes(packet: protocol::Packet) -> Vec<u8> {
    protocol::encode(&packet, &protocol::BedrockSession { shield_item_id: 0 })
        .unwrap()
        .to_vec()
}
fn drain(runtime: &mut UiRuntime) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    while flush_form_response(runtime, |packet| {
        packets.push(bytes(packet));
        Ok(())
    })
    .unwrap()
    {}
    packets
}

#[test]
fn decoded_element_button_form_selects_index_one_and_cancels_without_duplicate_response() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let json = r#"{"type":"form","title":"Menu 世界","content":"Select α β","elements":[{"type":"button","text":"First ✓","image":null},{"type":"button","text":"第二","image":null}]}"#;
    let varuint = |mut value: usize, output: &mut Vec<u8>| {
        while value >= 128 {
            output.push((value as u8) | 128);
            value >>= 7;
        }
        output.push(value as u8);
    };
    // The existing pinned form fixture uses header 100, then form id and
    // length-prefixed JSON. This independent packet exercises actual decode.
    let mut packet = vec![100, 29];
    varuint(json.len(), &mut packet);
    packet.extend_from_slice(json.as_bytes());
    let mut encoded = vec![0xfe];
    varuint(packet.len(), &mut encoded);
    encoded.extend_from_slice(&packet);
    let mut decoded = protocol::decode_batch(
        encoded.into(),
        &protocol::BedrockSession { shield_item_id: 0 },
    )
    .unwrap();
    let Some(protocol::WorldEvent::Ui(event)) =
        protocol::into_world_event(decoded.pop().unwrap(), 0).unwrap()
    else {
        panic!("decoded form UI event")
    };
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(&mut player_runtime, envelope(1, 1, event.clone()))
        .unwrap();
    let ServerFormModel::TextMenu(menu) = &runtime.server_forms().active().unwrap().model else {
        panic!("decoded element menu must retain actionable buttons")
    };
    assert_eq!(menu.buttons[1].as_ref(), "第二");
    runtime
        .respond_to_server_form(identity(&runtime), LocalFormAction::SubmitButton(1))
        .unwrap();
    assert_eq!(
        drain(&mut runtime),
        vec![bytes(protocol::modal_form_submit_response(
            29,
            protocol::ModalFormResponseSelection::ButtonIndex(1)
        ))]
    );
    assert!(drain(&mut runtime).is_empty());
    runtime
        .apply(&mut player_runtime, envelope(1, 2, event))
        .unwrap();
    runtime
        .respond_to_server_form(identity(&runtime), LocalFormAction::Dismiss)
        .unwrap();
    assert_eq!(
        drain(&mut runtime),
        vec![bytes(protocol::modal_form_cancel_response(29))]
    );
    assert!(drain(&mut runtime).is_empty());
}

#[test]
fn different_id_overlap_is_busy_not_fifo_display_and_queue_is_bounded() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player_runtime, retained(1, 1)).unwrap();
    for id in 2..=11 {
        runtime
            .apply(&mut player_runtime, retained(id, u64::from(id)))
            .unwrap();
    }
    assert_eq!(identity(&runtime).form_id, 1);
    assert_eq!(runtime.server_forms().entries().count(), 1);
    assert_eq!(
        runtime.server_forms().queued_busy_count(),
        MAX_RETAINED_SERVER_FORMS
    );
    assert_eq!(runtime.server_forms().dropped_over_capacity(), 2);
    let packets = drain(&mut runtime);
    assert_eq!(packets[0], bytes(protocol::modal_form_busy_response(2)));
    assert_eq!(packets.len(), 8);
    assert_eq!(identity(&runtime).form_id, 1);
}

/// Queued busy replies must not trickle out one per frame.
#[test]
fn every_queued_busy_reply_leaves_in_one_flush() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player_runtime, retained(1, 1)).unwrap();
    for id in 2..=4 {
        runtime
            .apply(&mut player_runtime, retained(id, u64::from(id)))
            .unwrap();
    }
    assert_eq!(runtime.server_forms().queued_busy_count(), 3);
    let mut packets = Vec::new();
    assert!(
        flush_form_response(&mut runtime, |packet| {
            packets.push(bytes(packet));
            Ok(())
        })
        .unwrap()
    );
    assert_eq!(
        packets,
        (2..=4)
            .map(|id| bytes(protocol::modal_form_busy_response(id)))
            .collect::<Vec<_>>()
    );
    assert_eq!(runtime.server_forms().queued_busy_count(), 0);
}

#[test]
fn same_id_reissue_invalidates_full_answer_and_stale_actions() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player_runtime, retained(7, 1)).unwrap();
    let old = identity(&runtime);
    runtime
        .respond_to_server_form(old, LocalFormAction::SubmitButton(1))
        .unwrap();
    assert_eq!(
        flush_form_response(&mut runtime, |_| Err(FormTransportError::Full)),
        Err(FormTransportError::Full)
    );
    runtime.apply(&mut player_runtime, retained(7, 2)).unwrap();
    let new = identity(&runtime);
    assert_ne!(old, new);
    assert_eq!(
        runtime.respond_to_server_form(old, LocalFormAction::Dismiss),
        Err(FormRespondError::StaleIdentity)
    );
    assert!(drain(&mut runtime).is_empty());
    runtime
        .respond_to_server_form(new, LocalFormAction::SubmitButton(0))
        .unwrap();
    assert_eq!(
        drain(&mut runtime),
        vec![bytes(protocol::modal_form_submit_response(
            7,
            protocol::ModalFormResponseSelection::ButtonIndex(0)
        ))]
    );
}

#[test]
fn same_id_busy_reissue_removes_all_unsent_old_rejections() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player_runtime, retained(1, 1)).unwrap();
    runtime.apply(&mut player_runtime, retained(2, 2)).unwrap();
    assert_eq!(
        flush_form_response(&mut runtime, |_| Err(FormTransportError::Full)),
        Err(FormTransportError::Full)
    );
    runtime.apply(&mut player_runtime, retained(2, 3)).unwrap();
    assert_eq!(runtime.server_forms().queued_busy_count(), 1);
    runtime
        .respond_to_server_form(identity(&runtime), LocalFormAction::Dismiss)
        .unwrap();
    assert_eq!(drain(&mut runtime).len(), 2);
    runtime.apply(&mut player_runtime, retained(2, 4)).unwrap();
    assert_eq!(identity(&runtime).revision, 4);
    assert!(drain(&mut runtime).is_empty());
}

#[test]
fn accepted_answer_is_not_retried_or_retracted_by_id_reissue() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player_runtime, retained(7, 1)).unwrap();
    runtime
        .respond_to_server_form(identity(&runtime), LocalFormAction::SubmitButton(1))
        .unwrap();
    assert_eq!(drain(&mut runtime).len(), 1);
    runtime.apply(&mut player_runtime, retained(7, 2)).unwrap();
    assert!(drain(&mut runtime).is_empty());
    assert_eq!(identity(&runtime).revision, 2);
}

#[test]
fn new_displayed_revision_invalidates_queued_same_id_busy_reply() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player_runtime, retained(1, 1)).unwrap();
    runtime.apply(&mut player_runtime, retained(2, 2)).unwrap();
    runtime
        .respond_to_server_form(identity(&runtime), LocalFormAction::Dismiss)
        .unwrap();
    // The transport accepts the local answer and refuses the busy reply behind it.
    let mut accepted = 0;
    assert_eq!(
        flush_form_response(&mut runtime, |_| {
            accepted += 1;
            if accepted == 1 {
                Ok(())
            } else {
                Err(FormTransportError::Full)
            }
        }),
        Err(FormTransportError::Full)
    );
    assert_eq!(runtime.server_forms().queued_busy_count(), 1);
    runtime.apply(&mut player_runtime, retained(2, 3)).unwrap();
    assert_eq!(identity(&runtime).form_id, 2);
    assert_eq!(runtime.server_forms().queued_busy_count(), 0);
    assert!(
        drain(&mut runtime).is_empty(),
        "old busy response cannot cancel displayed replacement"
    );
}

#[test]
fn single_flight_index_validation_and_closed_transport_fail_closed() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player_runtime, retained(7, 1)).unwrap();
    let id = identity(&runtime);
    assert_eq!(
        runtime.respond_to_server_form(id, LocalFormAction::SubmitButton(2)),
        Err(FormRespondError::InvalidButton)
    );
    runtime
        .respond_to_server_form(id, LocalFormAction::SubmitButton(1))
        .unwrap();
    assert_eq!(
        runtime.respond_to_server_form(id, LocalFormAction::Dismiss),
        Err(FormRespondError::PendingResponse)
    );
    assert!(
        runtime.ui_focused(&player_runtime),
        "Full pending answers still own input"
    );
    assert_eq!(
        flush_form_response(&mut runtime, |_| Err(FormTransportError::Closed)),
        Err(FormTransportError::Closed)
    );
    assert!(drain(&mut runtime).is_empty());
    assert!(!runtime.ui_focused(&player_runtime));
}

#[test]
fn other_ui_busy_and_unsupported_cancel_are_honest() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    runtime.apply(&mut player_runtime, retained(7, 1)).unwrap();
    assert!(runtime.server_forms().active().is_none());
    assert_eq!(
        drain(&mut runtime),
        vec![bytes(protocol::modal_form_busy_response(7))]
    );
    let mut unsupported = retained(8, 2);
    if let UiEvent::Form(form) = &mut unsupported.event {
        form.model = ServerFormModel::Unsupported(UnsupportedForm::Controls);
    }
    runtime.close_chat();
    runtime.apply(&mut player_runtime, unsupported).unwrap();
    let id = identity(&runtime);
    assert_eq!(
        runtime.respond_to_server_form(id, LocalFormAction::CustomElements),
        Err(FormRespondError::CustomElementsUnsupported)
    );
    assert_eq!(
        runtime.respond_to_server_form(id, LocalFormAction::SubmitButton(0)),
        Err(FormRespondError::UnsupportedControls)
    );
    runtime
        .respond_to_server_form(id, LocalFormAction::Dismiss)
        .unwrap();
    assert_eq!(
        drain(&mut runtime),
        vec![bytes(protocol::modal_form_cancel_response(8))]
    );
}

#[test]
fn session_and_dimension_retirement_clear_every_unsent_response() {
    let mut player_runtime = player_state::PlayerState::new(1);

    for new_session in [false, true] {
        let mut runtime = UiRuntime::new(1);
        runtime.note_stream_dimension(0);
        runtime.apply(&mut player_runtime, retained(1, 1)).unwrap();
        runtime.apply(&mut player_runtime, retained(2, 2)).unwrap();
        runtime.note_stream_dimension(0);
        assert!(
            runtime.server_forms().active().is_some(),
            "the same dimension retains the form"
        );
        let old = identity(&runtime);
        runtime
            .respond_to_server_form(old, LocalFormAction::Dismiss)
            .unwrap();
        if new_session {
            player_runtime.begin_session(2);
            runtime.begin_session(2);
        } else {
            runtime.note_stream_dimension(1);
        }
        assert!(!runtime.ui_focused(&player_runtime));
        assert!(drain(&mut runtime).is_empty());
        assert_eq!(
            runtime.respond_to_server_form(old, LocalFormAction::Dismiss),
            Err(FormRespondError::StaleIdentity)
        );
    }
}

#[test]
fn epoch_identity_clears_unsent_state_but_never_reuses_a_rendered_revision() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.server_forms_mut().synchronize_epoch(1, 0);
    runtime.apply(&mut player_runtime, retained(7, 1)).unwrap();
    runtime.apply(&mut player_runtime, retained(8, 2)).unwrap();
    let old = identity(&runtime);
    runtime.server_forms_mut().move_focus(1);
    runtime.server_forms_mut().set_scroll(5);
    runtime
        .respond_to_server_form(old, LocalFormAction::Dismiss)
        .unwrap();
    assert_eq!(
        flush_form_response(&mut runtime, |_| Err(FormTransportError::Full)),
        Err(FormTransportError::Full)
    );
    runtime.server_forms_mut().synchronize_epoch(1, 0);
    assert!(
        runtime.server_forms().owns_input(),
        "unchanged authority preserves pending reply"
    );
    runtime.server_forms_mut().synchronize_epoch(1, 3);
    assert!(!runtime.server_forms().owns_input());
    assert_eq!(runtime.server_forms().queued_busy_count(), 0);
    assert_eq!(runtime.server_forms().focus(), 0);
    assert_eq!(runtime.server_forms().scroll(), 0);
    assert!(drain(&mut runtime).is_empty());
    runtime.apply(&mut player_runtime, retained(7, 4)).unwrap();
    assert!(identity(&runtime).revision > old.revision);
    assert_eq!(
        runtime.respond_to_server_form(old, LocalFormAction::Dismiss),
        Err(FormRespondError::StaleIdentity)
    );
    runtime.server_forms_mut().synchronize_epoch(2, 3);
    assert!(
        runtime.server_forms().active().is_none(),
        "session replacement fences the same token"
    );
}
