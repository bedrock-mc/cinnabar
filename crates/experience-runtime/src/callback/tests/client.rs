//! Client message ownership, staged payloads and shared callback budgets.

use super::{ACTOR, Fixture, LimitExceeded, Op, Scalar, WorldError, limited};
use crate::host::cinnabar::experience_server::types::{Scalar as Leaf, ValueNode as Node};
use crate::limits::{MAX_CLIENT_SEND_BYTES, MAX_CLIENT_SENDS, MAX_VALUE_DEPTH};

#[test]
fn send_reaches_only_the_actor() {
    let mut res = Fixture::new().res();
    let stranger = "00000000-0000-0000-0000-000000000000".to_owned();
    assert_eq!(
        res.send_client(stranger, "c".to_owned(), 1, vec![])
            .unwrap(),
        Err(WorldError::Denied)
    );
    let mut res = Fixture {
        actor: None,
        ..Fixture::new()
    }
    .res();
    assert_eq!(
        res.send_client(ACTOR.to_owned(), "c".to_owned(), 1, vec![])
            .unwrap(),
        Err(WorldError::PlayerUnavailable)
    );
}

/// The byte limit holds the whole callback's channels and payloads, as JSON; the limit itself
/// fits, and a refused send stages nothing.
#[test]
fn send_bytes_limit_is_inclusive_and_cumulative() {
    let mut res = Fixture::new().res();
    let mut send = |channel: &str, payload: Vec<Node>| {
        res.send_client(ACTOR.to_owned(), channel.to_owned(), 1, payload)
            .unwrap()
    };
    let text = |len: usize| vec![Node::Leaf(Leaf::Text("x".repeat(len)))];
    let fill = MAX_CLIENT_SEND_BYTES - "c".len() - r#"[{"type":"text","value":""}]"#.len();
    assert_eq!(send("c", text(fill + 1)), Err(WorldError::TooLarge));
    assert_eq!(send("c", text(fill)), Ok(()));
    assert_eq!(send("", vec![]), Err(WorldError::TooLarge));
    assert_eq!(res.ops.len(), 1);
}

/// The payload's nodes are its values in pre-order: a list or record header takes the next
/// values, as many as it counts, for its items, and the rest are the payload's later fields.
#[test]
fn send_stages_the_value_tree_of_its_nodes() {
    let mut res = Fixture::new().res();
    let nodes = vec![
        Node::List(2),
        Node::Record(2),
        Node::Leaf(Leaf::Integer(1)),
        Node::Leaf(Leaf::Text("a".to_owned())),
        Node::Record(0),
        Node::Leaf(Leaf::Bool(true)),
        Node::List(1),
        Node::Leaf(Leaf::Choice(2)),
    ];
    assert_eq!(
        res.send_client(ACTOR.to_owned(), "c".to_owned(), 3, nodes)
            .unwrap(),
        Ok(())
    );
    let tree = vec![
        Scalar::List(vec![
            Scalar::Record(vec![Scalar::Integer(1), Scalar::Text("a".to_owned())]),
            Scalar::Record(Vec::new()),
        ]),
        Scalar::Bool(true),
        Scalar::List(vec![Scalar::Choice(2)]),
    ];
    assert_eq!(
        res.ops,
        [Op::SendClient {
            control: false,
            player: ACTOR.to_owned(),
            channel: "c".to_owned(),
            schema: 3,
            payload: tree,
        }]
    );
}

/// Control replies keep the actor and payload checks while selecting retained delivery.
#[test]
fn control_send_stages_retained_delivery() {
    let mut res = Fixture::new().res();
    assert_eq!(
        res.send_control("other".into(), "c".into(), 1, vec![])
            .unwrap(),
        Err(WorldError::Denied)
    );
    assert!(res.ops.is_empty());
    assert_eq!(
        res.send_control(
            ACTOR.into(),
            "c".into(),
            1,
            vec![Node::Leaf(Leaf::Bool(true))]
        )
        .unwrap(),
        Ok(())
    );
    assert_eq!(
        res.ops,
        [Op::SendClient {
            control: true,
            player: ACTOR.into(),
            channel: "c".into(),
            schema: 1,
            payload: vec![Scalar::Bool(true)]
        }]
    );
}

/// Lists and records nest at most `MAX_VALUE_DEPTH` deep, a top-level one being level 1; a
/// deeper payload is too large and stages nothing.
#[test]
fn send_depth_limit_is_inclusive() {
    let nested = |depth: usize| {
        let mut nodes = vec![Node::List(1); depth];
        nodes.push(Node::Leaf(Leaf::Bool(true)));
        nodes
    };
    let mut res = Fixture::new().res();
    let mut send = |nodes| {
        res.send_client(ACTOR.to_owned(), "c".to_owned(), 1, nodes)
            .unwrap()
    };
    assert_eq!(send(nested(MAX_VALUE_DEPTH + 1)), Err(WorldError::TooLarge));
    assert_eq!(send(nested(MAX_VALUE_DEPTH)), Ok(()));
    assert_eq!(res.ops.len(), 1);
}

/// A header that counts more items than follow it is no payload at all: the guest's encoding is
/// broken, so the call traps.
#[test]
fn send_of_a_header_without_its_items_traps() {
    let mut res = Fixture::new().res();
    let mut send = |nodes| res.send_client(ACTOR.to_owned(), "c".to_owned(), 1, nodes);
    for nodes in [
        vec![Node::List(1)],
        vec![Node::Record(3), Node::Leaf(Leaf::Bool(true)), Node::List(0)],
        vec![Node::List(u32::MAX), Node::Leaf(Leaf::Bool(true))],
    ] {
        let error = send(nodes).unwrap_err();
        assert!(!error.is::<LimitExceeded>(), "{error:#}");
    }
    assert!(res.ops.is_empty());
}

#[test]
fn send_past_the_cap_traps() {
    let mut res = Fixture::new().res();
    let mut send = || res.send_client(ACTOR.to_owned(), "c".to_owned(), 1, vec![]);
    for _ in 0..MAX_CLIENT_SENDS {
        assert_eq!(send().unwrap(), Ok(()));
    }
    assert!(limited(send()));
}
