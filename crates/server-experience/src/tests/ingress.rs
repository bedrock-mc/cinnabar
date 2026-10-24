use super::*;
use std::collections::BTreeMap;

/// Builds two recipients with a payload channel suitable for count and byte queue tests.
fn queue_fixture() -> (negotiation::Grant, runtime::Capabilities, wire::Envelope) {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let mut offer = offer(&key);
    offer
        .scope
        .permissions
        .insert(manifest::Permission::Messaging);
    offer.packages[0].id = "demo".into();
    let mut other = offer.packages[0].clone();
    other.id = "other".into();
    offer.packages.push(other);
    let grant = negotiation::Grant {
        offer: negotiation::VerifiedOffer {
            offer,
            digest: String::new(),
        },
        session: "session".into(),
        connection: "connection".into(),
        subclient: 0,
        expires_unix: 1500,
        wire: negotiation::Wire::v1(),
    };
    let capabilities = runtime::Capabilities {
        scope: grant.offer.offer.scope.clone(),
        assets: BTreeSet::new(),
        templates: BTreeSet::new(),
        actions: BTreeSet::new(),
        max_message_bytes: grant.wire.limits.max_message_bytes,
        channels: ["demo.events", "demo.registry", "other.events"]
            .map(|id| wire::Channel {
                id: id.into(),
                schema: 1,
                direction: wire::Direction::ToClient,
                fields: vec![wire::Field::Text {
                    max_bytes: policy::MAX_PAYLOAD_BYTES as u16,
                }],
            })
            .to_vec(),
    };
    let message = wire::Envelope {
        version: grant.wire.version,
        session: grant.session.clone(),
        connection: grant.connection.clone(),
        subclient: 0,
        bundle: "demo".into(),
        generation: policy::INITIAL_BUNDLE_GENERATION,
        channel: "demo.events".into(),
        schema: 1,
        sequence: 1,
        world_epoch: 1,
        payload: vec![wire::Scalar::Text(String::new())],
    };
    (grant, capabilities, message)
}

#[test]
fn confirmation_can_pass_waiting_messages_without_consuming_them() {
    let (grant, capabilities, mut message) = queue_fixture();
    let mut ingress = wire::Ingress::new(0);
    for (sequence, channel) in [(1, "demo.events"), (2, "demo.events"), (3, "demo.registry")] {
        message.sequence = sequence;
        message.channel = channel.into();
        ingress
            .receive(&serde_json::to_vec(&message).unwrap(), 0, 0, &grant, |_| {
                Some(&capabilities)
            })
            .unwrap();
    }
    assert_eq!(
        ingress
            .peek_matching(0, 1, |message| message.channel == "demo.registry")
            .unwrap()
            .sequence,
        3
    );
    assert_eq!(ingress.peek(0, 1).unwrap().sequence, 1);
    ingress.acknowledge(3).unwrap();
    assert_eq!(ingress.pop(0, 1).unwrap().sequence, 1);
    assert_eq!(ingress.pop(0, 1).unwrap().sequence, 2);
    assert!(ingress.peek(0, 1).is_none());
}

#[test]
fn world_change_discards_waiting_messages_before_selecting_confirmation() {
    let (grant, capabilities, mut message) = queue_fixture();
    let mut ingress = wire::Ingress::new(0);
    for (sequence, epoch, publication, channel) in [
        (1, 1, 0, "demo.events"),
        (2, 1, 0, "demo.registry"),
        (3, 2, 0, "demo.events"),
        (4, 2, 1, "demo.registry"),
    ] {
        message.sequence = sequence;
        message.world_epoch = epoch;
        message.channel = channel.into();
        ingress
            .receive(
                &serde_json::to_vec(&message).unwrap(),
                0,
                publication,
                &grant,
                |_| Some(&capabilities),
            )
            .unwrap();
    }
    assert!(
        ingress
            .peek_matching(0, 2, |message| message.channel == "demo.registry")
            .is_none()
    );
    assert_eq!(ingress.stale, 2);
    assert_eq!(
        ingress
            .peek_matching(1, 2, |message| message.channel == "demo.registry")
            .unwrap()
            .sequence,
        4
    );
    ingress.acknowledge(4).unwrap();
    assert_eq!(ingress.pop(1, 2).unwrap().sequence, 3);
    assert!(ingress.pop(1, 2).is_none());
}

#[test]
fn count_and_byte_overflow_retire_only_the_recipient_and_release_capacity() {
    for payload_bytes in [0, 4000] {
        let (grant, capabilities, mut message) = queue_fixture();
        let mut ingress = wire::Ingress::new(0);
        message.bundle = "other".into();
        message.channel = "other.events".into();
        ingress
            .receive(&serde_json::to_vec(&message).unwrap(), 0, 0, &grant, |_| {
                Some(&capabilities)
            })
            .unwrap();
        message.bundle = "demo".into();
        message.channel = "demo.events".into();
        message.payload = vec![wire::Scalar::Text("x".repeat(payload_bytes))];
        let mut retained = 0;
        loop {
            message.sequence += 1;
            let result = ingress.receive(
                &serde_json::to_vec(&message).unwrap(),
                message.sequence * 1000,
                0,
                &grant,
                |_| Some(&capabilities),
            );
            if let Err(error) = result {
                assert_eq!(
                    error.downcast_ref::<wire::QueueOverflow>().unwrap().bundle,
                    "demo"
                );
                break;
            }
            retained += 1;
        }
        if payload_bytes == 0 {
            assert_eq!(retained, policy::MAX_QUEUE_MESSAGES - 1);
        } else {
            assert!(retained > 0 && retained < policy::MAX_QUEUE_MESSAGES - 1);
        }
        assert_eq!(ingress.pop(0, 1).unwrap().bundle, "other");
        assert!(ingress.pop(0, 1).is_none());
        message.sequence += 1;
        ingress
            .receive(
                &serde_json::to_vec(&message).unwrap(),
                message.sequence * 1000,
                0,
                &grant,
                |_| Some(&capabilities),
            )
            .unwrap();
        assert!(
            ingress.pop(0, 1).is_none(),
            "overflow is terminal for its recipient"
        );
        assert_eq!(ingress.skipped, 1);
        message.sequence += 1;
        message.bundle = "other".into();
        message.channel = "other.events".into();
        ingress
            .receive(
                &serde_json::to_vec(&message).unwrap(),
                message.sequence * 1000,
                0,
                &grant,
                |_| Some(&capabilities),
            )
            .unwrap();
        assert_eq!(ingress.pop(0, 1).unwrap().sequence, message.sequence);
    }
}

/// A stopped recipient's remaining fragments consume their sequence without buffering data
/// or discarding another recipient's accepted messages.
#[test]
fn fragmented_byte_overflow_drains_the_message_and_preserves_other_recipients() {
    let (mut grant, capabilities, mut message) = queue_fixture();
    grant.wire = negotiation::Wire {
        version: policy::MAX_WIRE_VERSION,
        limits: negotiation::Limits {
            max_fragment_bytes: 1024,
            max_message_bytes: 8192,
            max_reassembly_bytes: 8192,
        },
    };
    message.version = grant.wire.version;
    let mut ingress = wire::Ingress::new(0);
    message.bundle = "other".into();
    message.channel = "other.events".into();
    message.payload = vec![wire::Scalar::Text("x".repeat(3000))];
    for bytes in wire::encode(&message, &grant.wire).unwrap() {
        ingress
            .receive(&bytes, 0, 0, &grant, |_| Some(&capabilities))
            .unwrap();
    }
    message.sequence = 2;
    message.bundle = "demo".into();
    message.channel = "demo.events".into();
    message.payload = vec![wire::Scalar::Text("y".repeat(6000))];
    let mut overflows = 0;
    for bytes in wire::encode(&message, &grant.wire).unwrap() {
        if let Err(error) = ingress.receive(&bytes, 1000, 0, &grant, |_| Some(&capabilities)) {
            assert_eq!(
                error.downcast_ref::<wire::QueueOverflow>().unwrap().bundle,
                "demo"
            );
            overflows += 1;
        }
    }
    assert_eq!(overflows, 1);
    assert_eq!(ingress.pop(0, 1).unwrap().bundle, "other");
    assert!(ingress.pop(0, 1).is_none());
    message.sequence = 3;
    message.bundle = "other".into();
    message.channel = "other.events".into();
    message.payload = vec![wire::Scalar::Text("next".into())];
    for bytes in wire::encode(&message, &grant.wire).unwrap() {
        ingress
            .receive(&bytes, 2000, 0, &grant, |_| Some(&capabilities))
            .unwrap();
    }
    assert_eq!(ingress.pop(0, 1).unwrap().sequence, 3);
}

#[test]
fn channel_field_count_uses_the_shared_contract_limit() {
    let mut channel = wire::Channel {
        id: "fixture.events".into(),
        schema: policy::API_VERSION,
        direction: wire::Direction::ToClient,
        fields: vec![wire::Field::Bool {}; policy::MAX_CHANNEL_FIELDS],
    };
    let mut payload = vec![wire::Scalar::Bool(true); policy::MAX_CHANNEL_FIELDS];
    channel
        .validate(
            &payload,
            wire::Direction::ToClient,
            policy::MAX_PAYLOAD_BYTES,
        )
        .unwrap();
    channel.fields.push(wire::Field::Bool {});
    payload.push(wire::Scalar::Bool(true));
    assert!(
        channel
            .validate(
                &payload,
                wire::Direction::ToClient,
                policy::MAX_PAYLOAD_BYTES,
            )
            .is_err()
    );
}

#[test]
fn overlapping_package_names_do_not_share_permissions_or_channel_schemas() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let mut offer = offer(&key);
    offer
        .scope
        .permissions
        .insert(manifest::Permission::Messaging);
    offer.packages[0].id = "a".into();
    let mut child = offer.packages[0].clone();
    child.id = "a.b".into();
    offer.packages.push(child);
    let grant = negotiation::Grant {
        offer: negotiation::VerifiedOffer {
            offer,
            digest: String::new(),
        },
        session: "session".into(),
        connection: "connection".into(),
        subclient: 0,
        expires_unix: 1500,
        wire: negotiation::Wire::v1(),
    };
    let channel = wire::Channel {
        id: "a.b.events".into(),
        schema: 1,
        direction: wire::Direction::ToClient,
        fields: vec![wire::Field::Bool {}],
    };
    let capabilities = runtime::Capabilities {
        scope: grant.offer.offer.scope.clone(),
        assets: BTreeSet::new(),
        templates: BTreeSet::new(),
        channels: vec![channel],
        actions: BTreeSet::new(),
        max_message_bytes: grant.wire.limits.max_message_bytes,
    };
    let mut parent = capabilities.clone();
    parent.channels.clear();
    let mut recipients: BTreeMap<String, _> =
        BTreeMap::from([("a".into(), parent), ("a.b".into(), capabilities)]);
    let mut message = wire::Envelope {
        version: policy::WIRE_VERSION,
        session: grant.session.clone(),
        connection: grant.connection.clone(),
        subclient: 0,
        bundle: "a".into(),
        generation: policy::INITIAL_BUNDLE_GENERATION,
        channel: "a.b.events".into(),
        schema: 1,
        sequence: 1,
        world_epoch: 1,
        payload: vec![wire::Scalar::Bool(true)],
    };
    let mut ingress = wire::Ingress::new(0);
    ingress
        .receive(&serde_json::to_vec(&message).unwrap(), 0, 0, &grant, |id| {
            recipients.get(id)
        })
        .unwrap();
    assert!(ingress.pop(0, 1).is_none());
    assert_eq!(ingress.skipped, 1);
    recipients.get_mut("a").unwrap().scope.permissions.clear();
    assert!(
        wire::Ingress::new(0)
            .receive(&serde_json::to_vec(&message).unwrap(), 0, 0, &grant, |id| {
                recipients.get(id)
            })
            .is_err()
    );
    message.bundle = "a.b".into();
    let mut ingress = wire::Ingress::new(0);
    ingress
        .receive(&serde_json::to_vec(&message).unwrap(), 0, 0, &grant, |id| {
            recipients.get(id)
        })
        .unwrap();
    assert_eq!(ingress.pop(0, 1).unwrap().bundle, "a.b");
}
