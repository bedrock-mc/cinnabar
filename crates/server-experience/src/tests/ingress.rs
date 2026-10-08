use super::*;
use std::collections::BTreeMap;

#[test]
fn channel_field_count_uses_the_shared_contract_limit() {
    let mut channel = wire::Channel {
        id: "fixture.events".into(),
        schema: policy::API_VERSION,
        direction: wire::Direction::ToClient,
        fields: vec![wire::Field::Bool; policy::MAX_CHANNEL_FIELDS],
    };
    let mut payload = vec![wire::Scalar::Bool(true); policy::MAX_CHANNEL_FIELDS];
    channel
        .validate(
            &payload,
            wire::Direction::ToClient,
            policy::MAX_PAYLOAD_BYTES,
        )
        .unwrap();
    channel.fields.push(wire::Field::Bool);
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
        fields: vec![wire::Field::Bool],
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
