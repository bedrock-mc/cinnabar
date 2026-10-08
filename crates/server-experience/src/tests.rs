mod bundles;
mod ingress;
mod wire_v2;

use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::collections::BTreeSet;

/// Builds a minimal valid advertisement without any network or local assets.
fn offer(key: &Ed25519KeyPair) -> manifest::Offer {
    manifest::Offer {
        version: policy::WIRE_VERSION,
        audience: "example.org:19132".into(),
        server_key: crypto::hex(key.public_key().as_ref()),
        revision: 3,
        expires_unix: 2000,
        scope: manifest::Scope {
            permissions: BTreeSet::new(),
            origins: BTreeSet::from(["https://example.org".into()]),
            memory_bytes: 0,
            gpu_bytes: 0,
        },
        packages: vec![manifest::PackageOffer {
            id: "example:cinema".into(),
            publisher_key: crypto::hex(key.public_key().as_ref()),
            digest: crypto::digest(b"bundle"),
            bytes: 6,
            url: "https://example.org/bundle".into(),
        }],
        fallback: "Use the normal lobby and poster".into(),
        carrier: protocol::EXPERIENCE_CHANNEL.into(),
    }
}

#[test]
fn signed_offer_checks_audience_expiry_key_and_canonical_bytes() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let value = offer(&key);
    let marker = negotiation::Marker {
        server_key: value.server_key.clone(),
        offer: crypto::sign(&value, crypto::OFFER_DOMAIN, &key).unwrap(),
    };
    let bytes = serde_json::to_vec(&marker).unwrap();
    let verified = negotiation::VerifiedOffer::read(&bytes, &value.audience, 1000).unwrap();
    assert_eq!(verified.offer, value);
    assert!(negotiation::VerifiedOffer::read(&bytes, "elsewhere:19132", 1000).is_err());
    assert!(negotiation::VerifiedOffer::read(&bytes, &value.audience, value.expires_unix).is_err());
    assert!(
        marker
            .offer
            .verify::<manifest::Offer>(
                &crypto::hex(&[0; 32]),
                crypto::OFFER_DOMAIN,
                policy::MAX_MARKER_BYTES
            )
            .is_err()
    );
    assert!(
        marker
            .offer
            .verify::<manifest::Offer>(
                &value.server_key,
                crypto::ACCEPT_DOMAIN,
                policy::MAX_MARKER_BYTES
            )
            .is_err()
    );
}

#[test]
fn accept_is_bound_to_the_fresh_connection_and_exact_offer() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let value = offer(&key);
    let verified = negotiation::VerifiedOffer {
        digest: crypto::digest(&serde_json::to_vec(&value).unwrap()),
        offer: value,
    };
    let pending = negotiation::Pending::approve(verified.clone(), 0, 0).unwrap();
    let accept = negotiation::Accept {
        hello: pending.hello().clone(),
        server_challenge: crypto::hex(&[2; 32]),
        session: crypto::hex(&[3; 32]),
        audience: verified.offer.audience.clone(),
        offer_digest: verified.digest.clone(),
        revision: verified.offer.revision,
        expires_unix: 1500,
        wire: None,
    };
    let document = crypto::sign(&accept, crypto::ACCEPT_DOMAIN, &key).unwrap();
    assert!(pending.accept(&document, 1000, 1).is_ok());
    let second = negotiation::Pending::approve(verified, 0, 0).unwrap();
    assert!(second.accept(&document, 1000, 1).is_err());
}

#[test]
fn ssrf_and_rate_limits_are_conservative() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "100.64.0.1",
        "::1",
        "::ffff:8.8.8.8",
        "2002:0808:0808::1",
    ] {
        assert!(
            !fetch::public_address(address.parse().unwrap()),
            "{address}"
        );
    }
    let mut rate = wire::RateLimit::new(1000);
    for _ in 0..policy::MAX_MESSAGES_PER_SECOND {
        rate.charge(1, 1000).unwrap();
    }
    assert!(rate.charge(1, 999).is_err());
    rate.charge(1, 2000).unwrap();
    assert!(
        rate.charge(policy::MAX_BYTES_PER_SECOND as usize, 2000)
            .is_err()
    );
}

#[test]
fn cache_revalidates_corruption_and_never_uses_a_url_as_a_path() {
    let root = tempfile::tempdir().unwrap();
    let cache = cache::BundleCache::open(root.path()).unwrap();
    let hash = crypto::digest(b"bundle");
    cache.publish(&hash, b"bundle").unwrap();
    assert_eq!(cache.read(&hash).unwrap().unwrap(), b"bundle");
    std::fs::write(root.path().join(format!("{hash}.cxb")), b"modified").unwrap();
    assert!(cache.read(&hash).unwrap().is_none());
    assert!(cache.read("../../token").is_err());
    assert!(cache.publish(&hash, b"modified").is_err());
}

#[test]
fn aggregate_budget_and_trap_quarantine_cannot_be_multiplied() {
    let mut budget = runtime::Budget::default();
    let owner = runtime::Principal {
        session: crypto::hex(&[1; 32]),
        bundle: "test:one".into(),
        generation: policy::INITIAL_BUNDLE_GENERATION,
    };
    budget
        .reserve(owner.clone(), policy::MAX_GUEST_MEMORY, 0)
        .unwrap();
    let mut other = owner.clone();
    other.bundle = "test:two".into();
    budget.reserve(other, policy::MAX_GUEST_MEMORY, 0).unwrap();
    let mut third = owner.clone();
    third.bundle = "test:three".into();
    assert!(budget.reserve(third, 1, 0).is_err());
    budget.begin_slice();
    budget.dispatch(&owner).unwrap();
    budget.dispatch(&owner).unwrap();
    assert!(budget.dispatch(&owner).is_err());
    budget.quarantine(&owner);
    assert!(budget.reserve(owner, 1, 0).is_err());
}

#[test]
fn stale_and_partially_invalid_transactions_never_publish() {
    let owner = runtime::Principal {
        session: crypto::hex(&[1; 32]),
        bundle: "test:one".into(),
        generation: policy::INITIAL_BUNDLE_GENERATION,
    };
    let capabilities = runtime::Capabilities {
        scope: manifest::Scope {
            permissions: BTreeSet::from([manifest::Permission::Ui]),
            origins: BTreeSet::new(),
            memory_bytes: 0,
            gpu_bytes: 0,
        },
        assets: BTreeSet::new(),
        templates: BTreeSet::new(),
        channels: Vec::new(),
        actions: BTreeSet::new(),
        max_message_bytes: policy::MAX_MESSAGE_BYTES as u32,
    };
    let transaction = runtime::Transaction {
        owner: owner.clone(),
        epoch: 1,
        commands: vec![
            runtime::Command::Widget {
                id: "status".into(),
                text: "valid".into(),
            },
            runtime::Command::Widget {
                id: "other".into(),
                text: "invalid\0".into(),
            },
        ],
    };
    let mut contributions = runtime::Contributions::default();
    assert!(
        contributions
            .apply(&transaction, &owner, 1, &capabilities)
            .is_err()
    );
    assert!(contributions.widgets.is_empty());
    assert!(
        contributions
            .apply(&transaction, &owner, 2, &capabilities)
            .is_err()
    );
}

#[test]
fn remembered_scope_requires_reapproval_and_updates_rollback_floor() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let mut value = offer(&key);
    let mut settings = trust::Settings::default();
    settings.remember(&value, trust::Decision::Always).unwrap();
    value.revision += 1;
    let marker = negotiation::Marker {
        server_key: value.server_key.clone(),
        offer: crypto::sign(&value, crypto::OFFER_DOMAIN, &key).unwrap(),
    };
    let bytes = serde_json::to_vec(&marker).unwrap();
    let mut session = session::Session::default();
    assert!(
        session
            .discover(&bytes, &value.audience, &mut settings, 1000, 0)
            .unwrap()
    );
    assert!(session.take_outbound().is_some());
    assert_eq!(settings.pins[0].highest_revision, value.revision);
    value.revision -= 1;
    assert_eq!(settings.decision(&value).unwrap(), None);
    value.revision += 1;
    value.scope.permissions.insert(manifest::Permission::Media);
    assert_eq!(settings.decision(&value).unwrap(), None);
    settings.remember(&value, trust::Decision::Never).unwrap();
    value.server_key = crypto::hex(&[1; 32]);
    assert_eq!(
        settings.decision(&value).unwrap(),
        Some(trust::Decision::Never)
    );
}

#[test]
fn canonical_signature_rejects_alternate_json_even_when_signed() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let value = offer(&key);
    let payload = serde_json::to_vec_pretty(&value).unwrap();
    let mut message = crypto::OFFER_DOMAIN.to_vec();
    message.extend_from_slice(&payload);
    let document = crypto::SignedDocument {
        payload: crypto::hex(&payload),
        signature: crypto::hex(key.sign(&message).as_ref()),
    };
    assert!(
        document
            .verify::<manifest::Offer>(
                &value.server_key,
                crypto::OFFER_DOMAIN,
                policy::MAX_MARKER_BYTES,
            )
            .is_err()
    );
}

#[test]
fn typed_records_wait_for_publication_and_replay_quarantines() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let mut value = offer(&key);
    value
        .scope
        .permissions
        .insert(manifest::Permission::Messaging);
    let grant = negotiation::Grant {
        offer: negotiation::VerifiedOffer {
            digest: crypto::digest(&serde_json::to_vec(&value).unwrap()),
            offer: value,
        },
        session: crypto::hex(&[2; 32]),
        connection: crypto::hex(&[3; 32]),
        subclient: 0,
        expires_unix: 1500,
        wire: negotiation::Wire::v1(),
    };
    let channel = wire::Channel {
        id: format!("{}.score", grant.offer.offer.packages[0].id),
        schema: 1,
        direction: wire::Direction::ToClient,
        fields: vec![wire::Field::Integer { min: 0, max: 100 }],
    };
    let message = wire::Envelope {
        version: policy::WIRE_VERSION,
        session: grant.session.clone(),
        connection: grant.connection.clone(),
        subclient: grant.subclient,
        bundle: grant.offer.offer.packages[0].id.clone(),
        generation: policy::INITIAL_BUNDLE_GENERATION,
        channel: channel.id.clone(),
        schema: channel.schema,
        sequence: 1,
        world_epoch: 7,
        payload: vec![wire::Scalar::Integer(42)],
    };
    let bytes = serde_json::to_vec(&message).unwrap();
    let recipients = std::collections::BTreeMap::from([(
        message.bundle.clone(),
        runtime::Capabilities {
            scope: grant.offer.offer.scope.clone(),
            assets: BTreeSet::new(),
            templates: BTreeSet::new(),
            channels: vec![channel.clone()],
            actions: BTreeSet::new(),
            max_message_bytes: grant.wire.limits.max_message_bytes,
        },
    )]);
    let mut ingress = wire::Ingress::new(0);
    ingress
        .receive(&bytes, 0, 100, &grant, |id| recipients.get(id))
        .unwrap();
    assert!(ingress.pop(99, 7).is_none());
    assert_eq!(ingress.pop(100, 7).unwrap().payload, message.payload);
    assert!(
        ingress
            .receive(&bytes, 0, 101, &grant, |id| recipients.get(id))
            .is_err()
    );
    assert!(ingress.pop(u64::MAX, 7).is_none());
}

#[test]
fn review_quarantine_survives_a_bundle_generation_change() {
    let mut budget = runtime::Budget::default();
    let mut owner = runtime::Principal {
        session: "session".into(),
        bundle: "bundle".into(),
        generation: 1,
    };
    budget.reserve(owner.clone(), 1, 1).unwrap();
    budget.quarantine(&owner);
    owner.generation += 1;
    assert!(budget.reserve(owner.clone(), 1, 1).is_err());
    owner.session = "replacement".into();
    assert!(budget.reserve(owner, 1, 1).is_ok());
}

#[test]
fn review_changing_trust_scope_preserves_the_prior_revision_floor() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let mut offered = offer(&key);
    let mut settings = trust::Settings::default();
    offered.revision = 10;
    settings
        .remember(&offered, trust::Decision::Always)
        .unwrap();
    let original = offered.clone();
    offered.revision = 11;
    offered.scope.permissions.insert(manifest::Permission::Ui);
    settings
        .remember(&offered, trust::Decision::Always)
        .unwrap();
    let mut rollback = original;
    rollback.revision = 9;
    assert!(
        settings
            .remember(&rollback, trust::Decision::Always)
            .is_err()
    );
}

#[test]
fn review_session_snapshots_share_single_use_handshake_authority() {
    for revoke in [false, true] {
        let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let value = offer(&key);
        let marker = negotiation::Marker {
            server_key: value.server_key.clone(),
            offer: crypto::sign(&value, crypto::OFFER_DOMAIN, &key).unwrap(),
        };
        let mut settings = trust::Settings::default();
        let mut session = session::Session::default();
        session
            .discover(
                &serde_json::to_vec(&marker).unwrap(),
                &value.audience,
                &mut settings,
                1000,
                0,
            )
            .unwrap();
        session
            .choose(trust::Choice::Once, &mut settings, 0)
            .unwrap();
        let session::Control::Hello(hello) =
            serde_json::from_slice(&session.take_outbound().unwrap()).unwrap()
        else {
            panic!("hello");
        };
        let accept = negotiation::Accept {
            audience: value.audience.clone(),
            offer_digest: hello.offer_digest.clone(),
            revision: value.revision,
            hello,
            server_challenge: crypto::hex(&[2; 32]),
            session: crypto::hex(&[3; 32]),
            expires_unix: 1500,
            wire: None,
        };
        let bytes = serde_json::to_vec(&session::Control::Accept(
            crypto::sign(&accept, crypto::ACCEPT_DOMAIN, &key).unwrap(),
        ))
        .unwrap();
        let mut snapshot = session.clone();
        if revoke {
            session.disable();
        } else {
            session.receive(&bytes, 1000, 1).unwrap();
        }
        assert!(snapshot.receive(&bytes, 1000, 1).is_err());
        assert!(matches!(snapshot.state, session::State::Disabled));
    }
}

/// A client part's strikes mirror the server adapter's: the same limit within the same window,
/// read from `tools/localserver/experience/limits.go`.
#[test]
fn guest_strikes_mirror_the_server_adapter() {
    let go = include_str!("../../../tools/localserver/experience/limits.go");
    let value = |name: &str| {
        go.lines()
            .find_map(|line| line.strip_prefix(&format!("const {name} = ")))
            .unwrap_or_else(|| panic!("limits.go declares {name}"))
            .trim()
    };
    assert_eq!(value("strikeLimit"), policy::MAX_GUEST_STRIKES.to_string());
    let window_ms = match value("strikeWindow") {
        "time.Minute" => 60_000,
        "time.Second" => 1_000,
        other => {
            let (count, unit) = other.split_once(" * time.").expect("N * time.Unit");
            let unit = match unit {
                "Minute" => 60_000,
                "Second" => 1_000,
                other => panic!("unit {other}"),
            };
            count.trim().parse::<u64>().unwrap() * unit
        }
    };
    assert_eq!(window_ms, policy::GUEST_STRIKE_WINDOW_MS);
}

/// `set-text` stages a bounded, plain text for an edit box by its `text_box_name`; each one the
/// modal keeps carries its own sequence, so a presenter applies it once and later typing stands.
#[test]
fn modal_texts_are_bounded_and_sequenced() {
    let owner = runtime::Principal {
        session: crypto::hex(&[1; 32]),
        bundle: "test:one".into(),
        generation: policy::INITIAL_BUNDLE_GENERATION,
    };
    let capabilities = runtime::Capabilities {
        scope: manifest::Scope {
            permissions: BTreeSet::from([manifest::Permission::ModalUi]),
            origins: BTreeSet::new(),
            memory_bytes: 0,
            gpu_bytes: 0,
        },
        assets: BTreeSet::new(),
        templates: BTreeSet::new(),
        channels: Vec::new(),
        actions: BTreeSet::new(),
        max_message_bytes: policy::MAX_MESSAGE_BYTES as u32,
    };
    let text = |control: &str, text: &str| runtime::Command::Text {
        control: control.into(),
        text: text.into(),
    };
    for refused in [
        text("Search", "iron"),
        text("search", &"x".repeat(policy::MAX_EDIT_TEXT_BYTES + 1)),
        text("search", "tab\there"),
    ] {
        assert!(capabilities.validate(&refused).is_err(), "{refused:?}");
    }
    let mut ui_only = capabilities.clone();
    ui_only.scope.permissions = BTreeSet::from([manifest::Permission::Ui]);
    assert!(ui_only.validate(&text("search", "iron")).is_err());
    let mut contributions = runtime::Contributions::default();
    let transaction = |commands| runtime::Transaction {
        owner: owner.clone(),
        epoch: 1,
        commands,
    };
    contributions
        .apply(
            &transaction(vec![text("search", "iron"), text("search", "")]),
            &owner,
            1,
            &capabilities,
        )
        .unwrap();
    let (first, empty) = contributions.modal.texts["search"].clone();
    assert_eq!(empty, "");
    contributions
        .apply(
            &transaction(vec![text("search", "gold\nbar")]),
            &owner,
            1,
            &capabilities,
        )
        .unwrap();
    let (second, gold) = contributions.modal.texts["search"].clone();
    assert!(second > first);
    assert_eq!(gold, "gold\nbar");
}
