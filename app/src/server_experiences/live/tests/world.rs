use super::*;

#[test]
fn native_epoch_spends_the_last_message_credit() {
    for byte_budget in [false, true] {
        let mut live = fixture_v2(1);
        sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 1);
        live.poll_ready(1, 0).unwrap();
        live.egress = RateLimit::new(0);
        let mut peer = RateLimit::new(0);
        let epoch = serde_json::to_vec(&Control::Epoch {
            session: live.grant.session.clone(),
            world_epoch: 2,
        })
        .unwrap();
        let sizes = if byte_budget {
            let remaining = MAX_BURST_BYTES as usize - epoch.len();
            let mut sizes = vec![MAX_PAYLOAD_BYTES; remaining / MAX_PAYLOAD_BYTES];
            sizes.push(remaining % MAX_PAYLOAD_BYTES);
            sizes
        } else {
            vec![1; MAX_MESSAGES_PER_SECOND as usize - 1]
        };
        for size in sizes {
            live.egress.charge(size, 0).unwrap();
            peer.charge(size, 0).unwrap();
        }
        let packets = live.poll_ready(2, 0).unwrap();
        assert_eq!(packets, [epoch]);
        peer.charge(packets[0].len(), 0).unwrap();
        assert_eq!(live.egress, peer);
        assert!(live.egress.charge(1, 0).is_err());
        assert!(peer.charge(1, 0).is_err());
        sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 2);
        assert!(live.poll_ready(2, 0).unwrap().is_empty());
        assert_eq!(live.pending_sends.len(), 1);
        let packets = live.poll_ready(2, 1000).unwrap();
        assert_eq!(packets.len(), 1);
        peer.charge(packets[0].len(), 1000).unwrap();
        assert_eq!(live.egress, peer);
    }
}

#[test]
fn admission_keeps_a_world_acknowledgment_through_a_later_native_epoch() {
    let mut live = fixture_v2(1);
    sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 1);
    live.poll_ready(1, 0).unwrap();
    let request = serde_json::to_vec(&Control::Epoch {
        session: live.grant.session.clone(),
        world_epoch: 2,
    })
    .unwrap();
    live.receive(&ExperienceMessage::new(request), 1, 1)
        .unwrap();
    // Admission applies the notice before the next poll; native travel then advances again.
    live.set_epoch(1).unwrap();
    let packets = live.poll_ready(2, CALLBACK_INTERVAL_MS).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::WorldAck {
            world_epoch: 3,
            requested_epoch: 2,
            ..
        }
    ));
}

#[test]
fn a_server_world_notice_advances_the_epoch_without_a_dimension_packet() {
    let mut live = fixture_v2(1);
    sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 1);
    live.poll_ready(1, 0).unwrap();
    assert!(live.ready);
    let request = serde_json::to_vec(&Control::Epoch {
        session: live.grant.session.clone(),
        world_epoch: 2,
    })
    .unwrap();
    live.receive(&ExperienceMessage::new(request), 1, live.native_epoch)
        .unwrap();
    let packets = live.poll_ready(1, CALLBACK_INTERVAL_MS).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::WorldAck {
            world_epoch: 2,
            requested_epoch: 2,
            ..
        }
    ));
    assert_eq!(live.epoch, 2);
    assert_eq!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched,
        vec![Event::Epoch]
    );
    live.poll_ready(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(live.epoch, 2);
    let packets = live.poll_ready(2, 3 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::Epoch { world_epoch: 3, .. }
    ));
}

#[test]
fn a_native_epoch_update_cannot_acknowledge_a_later_server_notice() {
    let mut live = fixture_v2(1);
    sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 1);
    live.poll_ready(1, 0).unwrap();
    let packets = live.poll_ready(2, CALLBACK_INTERVAL_MS).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::Epoch { world_epoch: 2, .. }
    ));
    for requested_epoch in [2, 3] {
        let notice = serde_json::to_vec(&Control::Epoch {
            session: live.grant.session.clone(),
            world_epoch: requested_epoch,
        })
        .unwrap();
        live.receive(
            &ExperienceMessage::new(notice),
            CALLBACK_INTERVAL_MS + 1,
            live.native_epoch,
        )
        .unwrap();
    }
    let packets = live.poll_ready(2, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::WorldAck {
            world_epoch: 3,
            requested_epoch: 3,
            ..
        }
    ));
    // A later notice can name an epoch the native update has already reached.
    live.poll_ready(3, 3 * CALLBACK_INTERVAL_MS).unwrap();
    let notice = serde_json::to_vec(&Control::Epoch {
        session: live.grant.session.clone(),
        world_epoch: 4,
    })
    .unwrap();
    live.receive(
        &ExperienceMessage::new(notice),
        3 * CALLBACK_INTERVAL_MS + 1,
        live.native_epoch,
    )
    .unwrap();
    let packets = live.poll_ready(3, 4 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::WorldAck {
            world_epoch: 5,
            requested_epoch: 4,
            ..
        }
    ));
}

#[test]
fn world_notices_cross_network_admission_with_exhausted_data_credits() {
    for byte_budget in [false, true] {
        let mut live = fixture_v2(1);
        let mut network = client_session::ExperienceIngress::default();
        sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 1);
        live.poll_ready(1, 0).unwrap();
        live.egress = RateLimit::new(0);
        let count = if byte_budget {
            MAX_BURST_BYTES / protocol::MAX_EXPERIENCE_ENVELOPE_BYTES as u64 + 1
        } else {
            MAX_MESSAGES_PER_SECOND
        };
        for sequence in 1..=count {
            let mut bytes = wire::encode(
                &Envelope {
                    version: live.grant.wire.version,
                    session: live.grant.session.clone(),
                    connection: live.grant.connection.clone(),
                    subclient: 0,
                    bundle: "bundle0".into(),
                    generation: INITIAL_BUNDLE_GENERATION,
                    channel: "bundle0.events0".into(),
                    schema: API_VERSION,
                    sequence,
                    world_epoch: 1,
                    payload: vec![Scalar::List(Vec::new())],
                },
                &live.grant.wire,
            )
            .unwrap()
            .remove(0);
            if byte_budget {
                let size = (MAX_BURST_BYTES as usize)
                    .saturating_sub(
                        (sequence as usize - 1) * protocol::MAX_EXPERIENCE_ENVELOPE_BYTES,
                    )
                    .min(protocol::MAX_EXPERIENCE_ENVELOPE_BYTES);
                bytes.resize(size, b' ');
            }
            live.egress.charge(bytes.len(), 0).unwrap();
            let admitted = network.admit(bytes, 0).unwrap();
            live.receive(&admitted, 0, live.native_epoch).unwrap();
        }
        for requested_epoch in [2, 3] {
            let notice = serde_json::to_vec(&Control::Epoch {
                session: live.grant.session.clone(),
                world_epoch: requested_epoch,
            })
            .unwrap();
            let mut oversized = notice.clone();
            oversized.resize(protocol::MAX_EXPERIENCE_CONTROL_BYTES + 1, b' ');
            assert!(network.admit(oversized, 0).is_none());
            assert!(!network.failed());
            let admitted = network.admit(notice.clone(), 0).unwrap();
            assert!(network.admit(notice.clone(), 0).is_none());
            assert!(!network.failed());
            live.receive(&admitted, 0, live.native_epoch).unwrap();
            let packets = live.poll_ready(1, 0).unwrap();
            assert!(matches!(
                serde_json::from_slice::<Control>(&packets[0]).unwrap(),
                Control::WorldAck { world_epoch, requested_epoch: requested, .. }
                    if world_epoch == requested_epoch && requested == requested_epoch
            ));
            // Producing an acknowledgment does not release the network slot before its write.
            assert!(network.admit(notice, 0).is_none());
            network.written(packets[0].clone());
            assert!(live.egress.charge(1, 0).is_err());
        }
        assert!(!network.failed());
        assert_eq!(live.epoch, 3);
        // Refusing excess control traffic leaves ordinary data admission usable after refill.
        assert!(network.admit(b"data".to_vec(), 1000).is_some());
    }
}

#[test]
fn world_notices_require_the_live_session_and_wire_v2() {
    for mut live in [fixture(1), fixture_v2(1)] {
        if live.grant.wire.version == WIRE_VERSION {
            complete(&mut live, "bundle0");
        } else {
            sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 1);
        }
        live.poll_ready(1, 0).unwrap();
        assert!(live.ready);
        let session = if live.grant.wire.version == WIRE_VERSION {
            live.grant.session.clone()
        } else {
            "another session".into()
        };
        let request = serde_json::to_vec(&Control::Epoch {
            session,
            world_epoch: 2,
        })
        .unwrap();
        assert!(
            live.receive(&ExperienceMessage::new(request), 1, live.native_epoch)
                .is_err()
        );
        assert_eq!(live.epoch, 1);
    }
}

/// Server world notices bind drawing to the logical epoch even when native travel has not run.
#[test]
fn a_server_world_notice_rebinds_drawing_before_the_epoch_callback() {
    let mut live = fixture_v2(1);
    complete(&mut live, "bundle0");
    live.poll_ready(1, 0).unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance
        .capabilities
        .scope
        .permissions
        .insert(Permission::WorldDraw);
    instance.helper.as_mut().unwrap().observe_epoch = true;
    let notice = serde_json::to_vec(&Control::Epoch {
        session: live.grant.session.clone(),
        world_epoch: 9,
    })
    .unwrap();
    live.receive(&ExperienceMessage::new(notice), 1, 1).unwrap();
    live.poll_ready(1, CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(live.native_epoch, 1);
    assert_eq!(live.epoch, 9);
    let helper = live.instances["bundle0"].helper.as_ref().unwrap();
    assert_eq!(helper.dispatched, [Event::Epoch]);
    assert_eq!(helper.contexts.last().unwrap().epoch, 9);
    assert_eq!(live.drawing_fence.unwrap().1, 9);
    live.poll_ready(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(live.instances["bundle0"].stopped.is_none());
    live.poll_ready(2, 3 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(live.epoch, 10);
    assert_eq!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .contexts
            .last()
            .unwrap()
            .epoch,
        10
    );
}
