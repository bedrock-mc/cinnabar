//! Form authority, overlap policy and definite-unsent backpressure witnesses.
use client_ui::ui_runtime::{
    FormRespondError, FormTransportError, LocalFormAction, MAX_RETAINED_SERVER_FORMS,
    SequencedUiEvent, ServerFormIdentity, UiRuntime, flush_form_response,
};
use protocol::UiEvent;

use super::fixtures::envelope;
use protocol::{FormKind, FormRequestEvent, ServerFormModel, TextMenuForm, UnsupportedForm};
use std::sync::Arc;

/// Builds a two-button form with ordered session metadata.
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
/// Reads the identity of the displayed form.
fn identity(runtime: &UiRuntime) -> ServerFormIdentity {
    runtime.server_forms().active().unwrap().identity
}
/// Returns the encoded packet bytes.
fn bytes(packet: protocol::Packet) -> Vec<u8> {
    protocol::encode(&packet).unwrap().to_vec()
}
/// Flushes every form response accepted by the fixture transport.
fn drain(runtime: &mut UiRuntime) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    while flush_form_response(
        runtime,
        |_| false,
        |packet| {
            packets.push(bytes(packet));
            Ok(())
        },
    )
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
    let mut decoded = protocol::decode_batch(encoded.into()).unwrap();
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
        flush_form_response(
            &mut runtime,
            |_| false,
            |packet| {
                packets.push(bytes(packet));
                Ok(())
            }
        )
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
fn same_id_reissue_preserves_full_answer_and_rejects_incoming_request() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player_runtime, retained(7, 1)).unwrap();
    let old = identity(&runtime);
    runtime
        .respond_to_server_form(old, LocalFormAction::SubmitButton(1))
        .unwrap();
    assert_eq!(
        flush_form_response(&mut runtime, |_| false, |_| Err(FormTransportError::Full)),
        Err(FormTransportError::Full)
    );
    runtime.apply(&mut player_runtime, retained(7, 2)).unwrap();
    assert!(runtime.server_forms().active().is_none());
    assert_eq!(
        drain(&mut runtime),
        vec![
            bytes(protocol::modal_form_submit_response(
                7,
                protocol::ModalFormResponseSelection::ButtonIndex(1)
            )),
            bytes(protocol::modal_form_busy_response(7)),
        ]
    );
    runtime.apply(&mut player_runtime, retained(7, 3)).unwrap();
    let new = identity(&runtime);
    assert_ne!(old, new);
    assert_eq!(
        runtime.respond_to_server_form(old, LocalFormAction::Dismiss),
        Err(FormRespondError::StaleIdentity)
    );
    assert!(drain(&mut runtime).is_empty());
}

#[test]
fn same_id_busy_reissue_removes_all_unsent_old_rejections() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player_runtime, retained(1, 1)).unwrap();
    runtime.apply(&mut player_runtime, retained(2, 2)).unwrap();
    assert_eq!(
        flush_form_response(&mut runtime, |_| false, |_| Err(FormTransportError::Full)),
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
        flush_form_response(
            &mut runtime,
            |_| false,
            |_| {
                accepted += 1;
                if accepted == 1 {
                    Ok(())
                } else {
                    Err(FormTransportError::Full)
                }
            }
        ),
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
        flush_form_response(&mut runtime, |_| false, |_| Err(FormTransportError::Closed)),
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
        flush_form_response(&mut runtime, |_| false, |_| Err(FormTransportError::Full)),
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

/// Normalizes a dialogue and binds its admitted actor lifetime.
fn active_npc_dialogue(unique_id: i64) -> UiRuntime {
    // NPCDialogue: header 169, fixed-width unique ID, Open, then four strings.
    let mut packet = vec![0xa9, 1];
    packet.extend_from_slice(&unique_id.to_le_bytes());
    packet.push(0);
    for text in [
        "Choose",
        "shop",
        "Trader",
        r#"[{"button_name":"Buy","mode":0}]"#,
    ] {
        assert!(text.len() < 128);
        packet.push(text.len() as u8);
        packet.extend_from_slice(text.as_bytes());
    }
    assert!(packet.len() < 128);
    let mut batch = vec![0xfe, packet.len() as u8];
    batch.extend_from_slice(&packet);
    let packet = protocol::decode_batch(batch.into()).unwrap().pop().unwrap();
    let Some(protocol::WorldEvent::Ui(event)) = protocol::into_world_event(packet, 0).unwrap()
    else {
        panic!("dialogue must normalize to a UI event");
    };
    let UiEvent::Form(form) = &event else {
        panic!("NPC form")
    };
    let ServerFormModel::NpcDialogue(npc) = &form.model else {
        panic!("NPC dialogue")
    };
    assert_eq!(npc.npc_unique_id, unique_id);
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.apply(&mut player, envelope(1, 1, event)).unwrap();
    let identity = identity(&runtime);
    runtime
        .server_forms_mut()
        .bind_npc_actor(identity, npc_actor());
    runtime
}

/// Retains one user response against the admitted dialogue.
fn pending_npc_response(unique_id: i64, action: LocalFormAction) -> UiRuntime {
    let mut runtime = active_npc_dialogue(unique_id);
    let identity = identity(&runtime);
    runtime.respond_to_server_form(identity, action).unwrap();
    runtime
}

/// Supplies the authoritative actor bound when this dialogue was admitted.
fn npc_actor() -> actor_animation::ActorLifetimeId {
    actor_animation::ActorLifetimeId {
        session_id: 1,
        dimension: 0,
        runtime_id: 99,
        spawn_revision: 7,
    }
}

#[test]
fn npc_replies_use_the_live_runtime_id_instead_of_the_dialogue_unique_id() {
    for (action, request) in [
        (
            LocalFormAction::Dismiss,
            protocol::NpcRequestKind::ExecuteClosingCommands,
        ),
        (
            LocalFormAction::SubmitButton(0),
            protocol::NpcRequestKind::ExecuteAction(0),
        ),
    ] {
        let mut runtime = pending_npc_response(-77, action);
        let mut packets = Vec::new();
        assert!(
            flush_form_response(
                &mut runtime,
                |actor| actor == npc_actor(),
                |packet| {
                    packets.push(bytes(packet));
                    Ok(())
                }
            )
            .unwrap()
        );
        assert_eq!(
            packets,
            [bytes(protocol::npc_request_packet(99, "shop", request))]
        );
    }
}

#[test]
fn npc_reply_retries_revalidate_the_original_actor_lifetime() {
    let mut runtime = pending_npc_response(-77, LocalFormAction::Dismiss);
    assert_eq!(
        flush_form_response(
            &mut runtime,
            |actor| actor == npc_actor(),
            |_| Err(FormTransportError::Full)
        ),
        Err(FormTransportError::Full)
    );
    assert!(runtime.server_forms().owns_input());
    // A new actor with reused numeric IDs does not own the original weak lifetime.
    let replacement = actor_animation::ActorLifetimeId {
        spawn_revision: 8,
        ..npc_actor()
    };
    assert!(
        !flush_form_response(
            &mut runtime,
            |actor| actor == replacement,
            |_| panic!("an actor replacement must retire the unsent reply")
        )
        .unwrap()
    );
    assert!(!runtime.server_forms().owns_input());
    assert!(
        !flush_form_response(
            &mut runtime,
            |_| true,
            |_| panic!("a retired reply must not return")
        )
        .unwrap()
    );

    let mut runtime = pending_npc_response(-77, LocalFormAction::SubmitButton(0));
    assert_eq!(
        flush_form_response(&mut runtime, |_| true, |_| Err(FormTransportError::Full)),
        Err(FormTransportError::Full)
    );
    let mut packets = Vec::new();
    assert!(
        flush_form_response(
            &mut runtime,
            |actor| actor == npc_actor(),
            |packet| {
                packets.push(bytes(packet));
                Ok(())
            }
        )
        .unwrap()
    );
    assert_eq!(
        packets,
        [bytes(protocol::npc_request_packet(
            99,
            "shop",
            protocol::NpcRequestKind::ExecuteAction(0)
        ))]
    );
    assert!(
        !flush_form_response(
            &mut runtime,
            |_| true,
            |_| panic!("accepted reply repeated")
        )
        .unwrap()
    );
}

#[test]
fn npc_reply_after_despawn_is_consumed_without_sending() {
    let mut runtime = pending_npc_response(-77, LocalFormAction::Dismiss);
    assert!(
        !flush_form_response(
            &mut runtime,
            |_| false,
            |_| panic!("a despawned NPC cannot receive a reply")
        )
        .unwrap()
    );
    assert!(!runtime.server_forms().owns_input());
}

/// A menu takeover closes the dialogue through its actor protocol, including retries.
#[test]
fn npc_menu_takeover_keeps_its_closing_reply_and_actor_lifetime() {
    for actor_alive in [true, false] {
        let mut runtime = active_npc_dialogue(-77);
        runtime.server_forms_mut().reject_active_busy();
        runtime.server_forms_mut().reject_active_busy();
        assert_eq!(
            flush_form_response(&mut runtime, |_| true, |_| Err(FormTransportError::Full)),
            Err(FormTransportError::Full)
        );
        let mut packets = Vec::new();
        let sent = flush_form_response(
            &mut runtime,
            |actor| actor_alive && actor == npc_actor(),
            |packet| {
                packets.push(bytes(packet));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(sent, actor_alive);
        let expected = if actor_alive {
            vec![bytes(protocol::npc_request_packet(
                99,
                "shop",
                protocol::NpcRequestKind::ExecuteClosingCommands,
            ))]
        } else {
            Vec::new()
        };
        assert_eq!(packets, expected);
        assert!(
            !flush_form_response(
                &mut runtime,
                |_| true,
                |_| panic!("duplicate closing reply")
            )
            .unwrap()
        );
    }
}

/// Unsupported NPC content has no valid modal-form identity to cancel or reject.
#[test]
fn npc_unsupported_dismissal_and_menu_takeover_send_no_modal_response() {
    for menu_takeover in [false, true] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = UiRuntime::new(1);
        runtime
            .apply(
                &mut player,
                envelope(
                    1,
                    1,
                    UiEvent::Form(FormRequestEvent {
                        form_id: protocol::NPC_DIALOGUE_FORM_ID,
                        kind: FormKind::NpcDialogue,
                        title: None,
                        json: Arc::from("{}"),
                        model: ServerFormModel::Unsupported(UnsupportedForm::Controls),
                    }),
                ),
            )
            .unwrap();
        if menu_takeover {
            runtime.server_forms_mut().reject_active_busy();
        } else {
            let identity = identity(&runtime);
            runtime
                .respond_to_server_form(identity, LocalFormAction::Dismiss)
                .unwrap();
        }
        assert!(
            runtime.server_forms().owns_input(),
            "local retirement captures input until the response phase"
        );
        assert!(
            !flush_form_response(
                &mut runtime,
                |_| true,
                |_| panic!("NPC emitted a modal-form response")
            )
            .unwrap()
        );
        assert!(!runtime.server_forms().owns_input());
    }
}
