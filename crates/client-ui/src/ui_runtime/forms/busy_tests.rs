//! Every rejected request keeps its own answer through transport backpressure.

use super::*;

/// Delivers an ordinary request while another local menu owns input.
fn busy_request(runtime: &mut UiRuntime, form_id: u32, sequence: u64) {
    let session = runtime.session_id();
    runtime.server_forms_mut().admit(
        FormRequestEvent {
            form_id,
            kind: FormKind::Unknown,
            title: None,
            json: "{}".into(),
            model: ServerFormModel::Unsupported(protocol::UnsupportedForm::Family),
        },
        sequence,
        session,
        true,
    );
}

#[test]
fn busy_burst_retains_every_request_including_repeated_ids_through_backpressure() {
    let mut runtime = UiRuntime::new(1);
    let ids = (0..24).map(|id| id % 11).collect::<Vec<_>>();
    for (sequence, &id) in ids.iter().enumerate() {
        busy_request(&mut runtime, id, sequence as u64);
    }
    assert_eq!(runtime.server_forms().queued_busy_count(), ids.len());
    let mut packets = Vec::new();
    for expected in 0..ids.len() {
        let mut accepted = false;
        assert_eq!(
            flush_form_response(
                &mut runtime,
                |_| false,
                |packet| {
                    if accepted {
                        return Err(FormTransportError::Full);
                    }
                    accepted = true;
                    packets.push(packet);
                    Ok(())
                }
            ),
            if expected + 1 == ids.len() {
                Ok(true)
            } else {
                Err(FormTransportError::Full)
            }
        );
        assert_eq!(
            runtime.server_forms().queued_busy_count(),
            ids.len() - expected - 1
        );
    }
    assert_eq!(
        packets,
        ids.into_iter()
            .map(modal_form_busy_response)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        flush_form_response(&mut runtime, |_| false, |_| panic!("duplicate answer")),
        Ok(false)
    );
}

#[test]
fn busy_queue_keeps_old_rejections_when_the_same_id_can_be_displayed() {
    let mut runtime = UiRuntime::new(1);
    busy_request(&mut runtime, 7, 1);
    let event = FormRequestEvent {
        form_id: 7,
        kind: FormKind::Unknown,
        title: None,
        json: "{}".into(),
        model: ServerFormModel::Unsupported(protocol::UnsupportedForm::Family),
    };
    runtime.server_forms_mut().admit(event, 2, 1, false);
    assert_eq!(
        runtime.server_forms().active().unwrap().identity.revision,
        2
    );
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
    assert_eq!(packets, [modal_form_busy_response(7)]);
    assert_eq!(
        runtime.server_forms().active().unwrap().identity.revision,
        2
    );
}

#[test]
fn opening_another_menu_retains_the_active_rejection_after_a_burst() {
    let mut runtime = UiRuntime::new(1);
    runtime.server_forms_mut().admit(
        FormRequestEvent {
            form_id: 99,
            kind: FormKind::Unknown,
            title: None,
            json: "{}".into(),
            model: ServerFormModel::Unsupported(protocol::UnsupportedForm::Family),
        },
        1,
        1,
        false,
    );
    for id in 0..16 {
        busy_request(&mut runtime, id, u64::from(id) + 2);
    }
    runtime.server_forms_mut().reject_active_busy();
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
        (0..16)
            .chain([99])
            .map(modal_form_busy_response)
            .collect::<Vec<_>>()
    );
}
