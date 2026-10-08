use super::*;
use negotiation::{Limits, Wire};
use wire::{Direction, Envelope, Field, Fragment, Ingress, Scalar};

/// A wire v2 session at the host's own ceilings.
fn v2() -> Wire {
    Wire {
        version: policy::MAX_WIRE_VERSION,
        limits: Limits::host(),
    }
}

fn grant(wire: Wire) -> negotiation::Grant {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let mut value = offer(&key);
    value
        .scope
        .permissions
        .insert(manifest::Permission::Messaging);
    negotiation::Grant {
        offer: negotiation::VerifiedOffer {
            digest: crypto::digest(&serde_json::to_vec(&value).unwrap()),
            offer: value,
        },
        session: crypto::hex(&[2; 32]),
        connection: crypto::hex(&[3; 32]),
        subclient: 0,
        expires_unix: 1500,
        wire,
    }
}

/// The terminal's shape: a list of (id, count, name) records.
fn items_field(max_items: u16) -> Field {
    Field::List {
        item: Box::new(Field::Record {
            fields: vec![
                Field::Text { max_bytes: 96 },
                Field::Integer {
                    min: 0,
                    max: i64::from(u32::MAX),
                },
                Field::Text { max_bytes: 256 },
            ],
        }),
        max_items,
    }
}

fn item(i: usize, name: &str) -> Scalar {
    Scalar::Record(vec![
        Scalar::Text(format!("minecraft:item_{i}")),
        Scalar::Integer(i as i64),
        Scalar::Text(name.to_owned()),
    ])
}

fn items_channel(grant: &negotiation::Grant) -> wire::Channel {
    wire::Channel {
        id: format!("{}.items", grant.offer.offer.packages[0].id),
        schema: 1,
        direction: Direction::ToClient,
        fields: vec![items_field(4096)],
    }
}

fn capabilities(grant: &negotiation::Grant) -> runtime::Capabilities {
    runtime::Capabilities {
        scope: grant.offer.offer.scope.clone(),
        assets: BTreeSet::new(),
        templates: BTreeSet::new(),
        channels: vec![items_channel(grant)],
        actions: BTreeSet::new(),
        max_message_bytes: grant.wire.limits.max_message_bytes,
    }
}

fn envelope(
    grant: &negotiation::Grant,
    sequence: u64,
    world_epoch: u64,
    items: Vec<Scalar>,
) -> Envelope {
    Envelope {
        version: grant.wire.version,
        session: grant.session.clone(),
        connection: grant.connection.clone(),
        subclient: grant.subclient,
        bundle: grant.offer.offer.packages[0].id.clone(),
        generation: policy::INITIAL_BUNDLE_GENERATION,
        channel: items_channel(grant).id,
        schema: 1,
        sequence,
        world_epoch,
        payload: vec![Scalar::List(items)],
    }
}

/// Enough multi-byte names that the payload needs several fragments.
fn large_items(count: usize) -> Vec<Scalar> {
    (0..count)
        .map(|i| item(i, &format!("Certus Quartz Crystal \u{e9}\u{1f600} #{i}")))
        .collect()
}

/// `large_items(count)` with the first name padded until the first cut falls inside a
/// multi-byte character.
fn misaligned(count: usize) -> Vec<Scalar> {
    (0..256)
        .map(|pad| {
            let mut items = large_items(count);
            items[0] = item(0, &"x".repeat(pad));
            items
        })
        .find(|items| {
            !serde_json::to_string(&[Scalar::List(items.clone())])
                .unwrap()
                .is_char_boundary(policy::MAX_PAYLOAD_BYTES)
        })
        .unwrap()
}

/// One receiving direction whose clock moves a second per message, so only the rule under
/// test can reject.
struct Receiver {
    ingress: Ingress,
    clock: u64,
}

impl Receiver {
    fn new() -> Self {
        Self {
            ingress: Ingress::new(0),
            clock: 0,
        }
    }

    /// Feeds carrier messages in order, stopping at the first rejection.
    fn feed(&mut self, grant: &negotiation::Grant, messages: &[Vec<u8>]) -> anyhow::Result<()> {
        let capabilities = capabilities(grant);
        for bytes in messages {
            self.clock += 1000;
            self.ingress
                .receive(bytes, self.clock, 0, grant, |_| Some(&capabilities))?;
        }
        Ok(())
    }
}

fn fragments(messages: &[Vec<u8>]) -> Vec<Fragment> {
    messages
        .iter()
        .map(|bytes| serde_json::from_slice(bytes).unwrap())
        .collect()
}

fn bytes(fragment: &Fragment) -> Vec<u8> {
    serde_json::to_vec(fragment).unwrap()
}

#[test]
fn list_and_record_values_are_checked_positionally_and_totally() {
    let grant = grant(v2());
    let channel = items_channel(&grant);
    let valid = vec![Scalar::List(vec![item(1, "Fluix"), item(2, "")])];
    channel
        .validate(&valid, Direction::ToClient, policy::MAX_MESSAGE_BYTES)
        .unwrap();
    channel
        .validate(
            &[Scalar::List(Vec::new())],
            Direction::ToClient,
            policy::MAX_MESSAGE_BYTES,
        )
        .unwrap();
    let rejected = [
        // One item over max_items.
        vec![Scalar::List(vec![item(0, "x"); 4097])],
        // A record of the wrong arity.
        vec![Scalar::List(vec![Scalar::Record(vec![Scalar::Text(
            "minecraft:stone".into(),
        )])])],
        // A nested value of the wrong type, out of range or too long.
        vec![Scalar::List(vec![Scalar::Record(vec![
            Scalar::Integer(1),
            Scalar::Integer(1),
            Scalar::Text(String::new()),
        ])])],
        vec![Scalar::List(vec![Scalar::Record(vec![
            Scalar::Text(String::new()),
            Scalar::Integer(-1),
            Scalar::Text(String::new()),
        ])])],
        vec![Scalar::List(vec![item(0, &"x".repeat(257))])],
        // A list where a record is declared, and the reverse.
        vec![Scalar::List(vec![Scalar::List(Vec::new())])],
        vec![Scalar::Record(vec![item(0, "x")])],
    ];
    for payload in rejected {
        assert!(
            channel
                .validate(&payload, Direction::ToClient, policy::MAX_MESSAGE_BYTES)
                .is_err(),
            "{payload:?}"
        );
    }
    let large = vec![Scalar::List(large_items(250))];
    let size = serde_json::to_vec(&large).unwrap().len();
    assert!(size > policy::MAX_PAYLOAD_BYTES);
    channel.validate(&large, Direction::ToClient, size).unwrap();
    assert!(
        channel
            .validate(&large, Direction::ToClient, size - 1)
            .is_err()
    );
}

#[test]
fn containers_nest_at_most_the_shared_depth() {
    let mut field = Field::Bool;
    for level in 1..=policy::MAX_FIELD_DEPTH + 1 {
        field = if level % 2 == 0 {
            Field::Record {
                fields: vec![field],
            }
        } else {
            Field::List {
                item: Box::new(field),
                max_items: 1,
            }
        };
        let mut value = Scalar::Bool(true);
        for inner in 1..=level {
            value = if inner % 2 == 0 {
                Scalar::Record(vec![value])
            } else {
                Scalar::List(vec![value])
            };
        }
        let channel = wire::Channel {
            id: "fixture.nested".into(),
            schema: 1,
            direction: Direction::ToClient,
            fields: vec![field.clone()],
        };
        let allowed = level <= policy::MAX_FIELD_DEPTH;
        assert_eq!(channel.declared(), allowed, "level {level}");
        assert_eq!(
            channel
                .validate(&[value], Direction::ToClient, policy::MAX_MESSAGE_BYTES)
                .is_ok(),
            allowed,
            "level {level}"
        );
    }
    let wide = wire::Channel {
        id: "fixture.wide".into(),
        schema: 1,
        direction: Direction::ToClient,
        fields: vec![Field::Record {
            fields: vec![Field::Bool; policy::MAX_CHANNEL_FIELDS + 1],
        }],
    };
    assert!(!wide.declared());
}

#[test]
fn accept_selects_an_offered_version_with_limits_never_above_the_hello() {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let value = offer(&key);
    let verified = negotiation::VerifiedOffer {
        digest: crypto::digest(&serde_json::to_vec(&value).unwrap()),
        offer: value,
    };
    let host = Limits::host();
    let lower = Limits {
        max_fragment_bytes: 1024,
        max_message_bytes: 4096,
        max_reassembly_bytes: 8192,
    };
    let above = |change: fn(&mut Limits)| {
        let mut limits = host;
        change(&mut limits);
        limits
    };
    let wire = |version, limits| Some(Wire { version, limits });
    let cases = [
        (None, Some(Wire::v1())),
        (wire(2, host), Some(v2())),
        (
            wire(2, lower),
            Some(Wire {
                version: 2,
                limits: lower,
            }),
        ),
        // v1 is selected by leaving the field out, so v1 Accepts keep their exact bytes.
        (wire(1, host), None),
        (wire(3, host), None),
        (wire(2, above(|l| l.max_fragment_bytes += 1)), None),
        (wire(2, above(|l| l.max_message_bytes += 1)), None),
        (wire(2, above(|l| l.max_reassembly_bytes += 1)), None),
        (wire(2, above(|l| l.max_fragment_bytes = 0)), None),
        (
            wire(2, above(|l| l.max_fragment_bytes = l.max_message_bytes + 1)),
            None,
        ),
    ];
    for (selected, expected) in cases {
        let pending = negotiation::Pending::approve(verified.clone(), 0, 0).unwrap();
        let hello = pending.hello().clone();
        let offered = hello.wire.clone().unwrap();
        assert_eq!(
            offered.versions,
            (policy::WIRE_VERSION..=policy::MAX_WIRE_VERSION).collect::<BTreeSet<_>>()
        );
        assert_eq!(offered.limits, host);
        let accept = negotiation::Accept {
            hello,
            server_challenge: crypto::hex(&[2; 32]),
            session: crypto::hex(&[3; 32]),
            audience: verified.offer.audience.clone(),
            offer_digest: verified.digest.clone(),
            revision: verified.offer.revision,
            expires_unix: 1500,
            wire: selected,
        };
        let document = crypto::sign(&accept, crypto::ACCEPT_DOMAIN, &key).unwrap();
        let granted = pending
            .accept(&document, 1000, 1)
            .ok()
            .map(|grant| grant.wire);
        assert_eq!(granted, expected, "{selected:?}");
    }
}

#[test]
fn records_over_the_inline_limit_fragment_only_under_v2_and_reassemble() {
    let grant = grant(v2());
    let message = envelope(&grant, 1, 7, misaligned(250));
    assert!(wire::encode(&message, &Wire::v1()).is_err());
    let v1 = self::grant(Wire::v1());
    assert!(wire::encode(&envelope(&v1, 1, 7, large_items(250)), &v1.wire).is_err());
    let messages = wire::encode(&message, &grant.wire).unwrap();
    assert!(messages.len() >= 3);
    let parts = fragments(&messages);
    let payload = serde_json::to_string(&message.payload).unwrap();
    assert_eq!(
        parts
            .iter()
            .map(|f| f.fragment.data.as_str())
            .collect::<String>(),
        payload
    );
    for (index, part) in parts.iter().enumerate() {
        assert_eq!(
            (part.fragment.index, part.fragment.count, part.sequence),
            (index as u32, parts.len() as u32, 1)
        );
        assert!(part.fragment.data.len() <= policy::MAX_PAYLOAD_BYTES);
    }
    // The multi-byte names put some cut inside a character, which moves back to its start.
    assert!(
        parts[..parts.len() - 1]
            .iter()
            .any(|f| f.fragment.data.len() < policy::MAX_PAYLOAD_BYTES)
    );
    let mut receiver = Receiver::new();
    receiver
        .feed(&grant, &messages[..messages.len() - 1])
        .unwrap();
    assert!(receiver.ingress.pop(u64::MAX, 7).is_none());
    receiver
        .feed(&grant, &messages[messages.len() - 1..])
        .unwrap();
    assert_eq!(
        receiver.ingress.pop(u64::MAX, 7).unwrap().payload,
        message.payload
    );
    // The whole message used one sequence number.
    let next = wire::encode(&envelope(&grant, 2, 7, vec![item(1, "small")]), &grant.wire).unwrap();
    assert_eq!(next.len(), 1);
    receiver.feed(&grant, &next).unwrap();
    assert_eq!(receiver.ingress.pop(u64::MAX, 7).unwrap().sequence, 2);
}

#[test]
fn malformed_interleaved_or_over_budget_fragments_quarantine_the_channel() {
    let grant = grant(v2());
    let messages = wire::encode(&envelope(&grant, 1, 7, large_items(250)), &grant.wire).unwrap();
    let parts = fragments(&messages);
    let other = wire::encode(&envelope(&grant, 1, 7, large_items(200)), &grant.wire).unwrap();
    let whole = wire::encode(&envelope(&grant, 1, 7, vec![item(1, "x")]), &grant.wire).unwrap();
    let changed = |index: usize, change: fn(&mut Fragment)| {
        let mut part = parts[index].clone();
        change(&mut part);
        bytes(&part)
    };
    let attacks: Vec<(&str, Vec<Vec<u8>>)> = vec![
        (
            "an envelope inside a message",
            vec![messages[0].clone(), whole[0].clone()],
        ),
        (
            "a second message inside a message",
            vec![messages[0].clone(), other[0].clone()],
        ),
        (
            "a missing middle fragment",
            vec![messages[0].clone(), messages[2].clone()],
        ),
        (
            "a repeated fragment",
            vec![messages[0].clone(), messages[0].clone()],
        ),
        ("a later fragment first", vec![messages[1].clone()]),
        (
            "a changed count",
            vec![messages[0].clone(), changed(1, |f| f.fragment.count += 1)],
        ),
        (
            "a changed channel",
            vec![messages[0].clone(), changed(1, |f| f.schema += 1)],
        ),
        (
            "a changed sequence",
            vec![messages[0].clone(), changed(1, |f| f.sequence += 1)],
        ),
        (
            "a changed epoch",
            vec![messages[0].clone(), changed(1, |f| f.world_epoch += 1)],
        ),
        (
            "a one-fragment message",
            vec![changed(0, |f| f.fragment.count = 1)],
        ),
        (
            "an index beyond the count",
            vec![changed(0, |f| f.fragment.index = f.fragment.count)],
        ),
        ("empty data", vec![changed(0, |f| f.fragment.data.clear())]),
        (
            "data over the fragment limit",
            vec![changed(0, |f| f.fragment.data.push(' '))],
        ),
        (
            "a replayed sequence",
            vec![whole[0].clone(), messages[0].clone()],
        ),
        (
            "a v1 version",
            vec![changed(0, |f| f.version = policy::WIRE_VERSION)],
        ),
        (
            "data that is not a record",
            vec![
                changed(0, |f| f.fragment.count = 2),
                changed(1, |f| f.fragment.count = 2),
            ],
        ),
    ];
    for (name, attack) in attacks {
        let mut receiver = Receiver::new();
        assert!(receiver.feed(&grant, &attack).is_err(), "{name}");
        assert!(receiver.feed(&grant, &whole).is_err(), "{name} quarantines");
    }

    // A message whose data grows past the per-message cap fails before it is buffered whole.
    let max = policy::MAX_PAYLOAD_BYTES;
    let mut overflow = parts[0].clone();
    overflow.fragment.count = u32::MAX;
    overflow.fragment.data = "x".repeat(max);
    let mut receiver = Receiver::new();
    let mut result = Ok(());
    let mut sent = 0;
    for index in 0..policy::MAX_MESSAGE_BYTES / max + 1 {
        overflow.fragment.index = index as u32;
        result = receiver.feed(&grant, &[bytes(&overflow)]);
        if result.is_err() {
            break;
        }
        sent += max;
    }
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("message too large")
    );
    assert!(sent <= policy::MAX_MESSAGE_BYTES);

    // Undelivered messages and the open one share the per-connection budget.
    let total = messages.iter().map(Vec::len).sum::<usize>() + whole[0].len();
    for (budget, accepted) in [(total, true), (total - 1, false)] {
        let mut tight = grant.clone();
        tight.wire.limits.max_reassembly_bytes = budget as u32;
        let mut receiver = Receiver::new();
        let first = wire::encode(&envelope(&tight, 1, 7, vec![item(1, "x")]), &tight.wire).unwrap();
        receiver.feed(&tight, &first).unwrap();
        let second = wire::encode(&envelope(&tight, 2, 7, large_items(250)), &tight.wire).unwrap();
        assert_eq!(receiver.feed(&tight, &second).is_ok(), accepted, "{budget}");
    }
}

#[test]
fn old_epoch_messages_are_dropped_and_counted_while_sequences_continue() {
    for wire in [Wire::v1(), v2()] {
        let grant = grant(wire);
        let mut receiver = Receiver::new();
        let mut messages = Vec::new();
        for (sequence, epoch) in [(1, 7), (2, 8), (3, 7)] {
            let items = if sequence == 3 && wire.version != policy::WIRE_VERSION {
                large_items(250)
            } else {
                vec![item(sequence as usize, "x")]
            };
            messages.extend(
                wire::encode(&envelope(&grant, sequence, epoch, items), &grant.wire).unwrap(),
            );
        }
        receiver.feed(&grant, &messages).unwrap();
        assert_eq!(receiver.ingress.pop(u64::MAX, 8).unwrap().sequence, 2);
        assert!(receiver.ingress.pop(u64::MAX, 8).is_none());
        assert_eq!((receiver.ingress.stale, receiver.ingress.skipped), (2, 0));
    }
}
