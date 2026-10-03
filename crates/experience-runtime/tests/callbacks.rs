//! Callbacks on the probe guest: what each behavior stages and how each call ends. The probe
//! selects a behavior by the interacted block's x; `p(x)` is that block and `up(x)` the one above.

mod common;

use common::{
    ACTOR, AIR, COUNTER, callback, cell, client_message, interact, outcome, p, send, tell, up,
};
use experience_runtime::limits::MAX_REASON_BYTES;
use experience_runtime::protocol::{
    Call, Cause, Cell, Change, Face, FailKind, Op, Outcome, Request, Scalar,
};

/// `request` with its snapshot cell at `new.pos` replaced by `new`.
fn with_cell(mut request: Request, new: Cell) -> Request {
    let Request::Callback { snapshot, .. } = &mut request else {
        unreachable!("a callback request");
    };
    let old = snapshot
        .iter_mut()
        .find(|cell| cell.pos == new.pos)
        .expect("the cell is in the snapshot");
    *old = new;
    request
}

fn committed(ops: Vec<Op>) -> Outcome {
    Outcome::Committed { ops }
}

#[test]
fn counter_commits_data_then_tell() {
    assert_eq!(
        outcome(&interact(0)),
        committed(vec![
            Op::SetBlockData {
                pos: p(0),
                data: Some("01000000".to_owned()),
            },
            tell("count 1"),
        ])
    );
}

#[test]
fn trap_after_staging_commits_nothing() {
    let outcome = outcome(&interact(1));
    assert!(
        matches!(
            outcome,
            Outcome::Failed {
                kind: FailKind::Trap,
                ..
            }
        ),
        "{outcome:?}"
    );
}

#[test]
fn guest_error_commits_nothing() {
    assert_eq!(
        outcome(&interact(10)),
        Outcome::Rejected {
            reason: "nope".to_owned()
        }
    );
}

/// The probe's reason is about 2 MiB of the 3-byte `€`. It is cut at the last char boundary
/// within the limit, so the result still fits in a frame.
#[test]
fn oversized_rejection_reason_is_cut_at_a_char_boundary() {
    let outcome = outcome(&interact(18));
    let Outcome::Rejected { reason } = &outcome else {
        panic!("{outcome:?}");
    };
    let expected = "€".repeat(MAX_REASON_BYTES / 3);
    // Lengths first, so a failure does not print megabytes.
    assert_eq!(reason.len(), expected.len());
    assert_eq!(*reason, expected);
}

#[test]
fn reads_see_staged_writes() {
    assert_eq!(
        outcome(&interact(5)),
        committed(vec![
            Op::SetBlock {
                pos: up(5),
                id: COUNTER.to_owned(),
            },
            tell(COUNTER),
        ])
    );
}

#[test]
fn read_outside_snapshot_is_denied() {
    assert_eq!(outcome(&interact(6)), committed(vec![tell("denied")]));
}

/// The probe's `set-block(up)` fails too, so nothing but the tell is staged.
#[test]
fn unloaded_cell_is_unavailable() {
    let unloaded = Cell {
        pos: up(5),
        loaded: false,
        id: String::new(),
        owned: false,
        data: None,
    };
    assert_eq!(
        outcome(&with_cell(interact(5), unloaded)),
        committed(vec![tell("unavailable")])
    );
}

#[test]
fn data_over_cap_is_too_large() {
    assert_eq!(outcome(&interact(7)), committed(vec![tell("too-large")]));
}

#[test]
fn data_over_budget_is_quota_exceeded() {
    let mut request = interact(0);
    let Request::Callback { data_budget, .. } = &mut request else {
        unreachable!("a callback request");
    };
    *data_budget = 0;
    assert_eq!(
        outcome(&request),
        committed(vec![tell("error quota-exceeded")])
    );
}

#[test]
fn vanilla_target_is_unknown_block() {
    assert_eq!(
        outcome(&interact(8)),
        committed(vec![tell("unknown-block")])
    );
}

#[test]
fn tell_to_non_actor_is_denied() {
    assert_eq!(outcome(&interact(9)), committed(vec![tell("denied")]));
}

/// `p` starts with data, so each write must replace it to be read back.
#[test]
fn none_and_empty_data_differ() {
    let request = |x| with_cell(interact(x), cell(p(x), COUNTER, true, Some("0a")));
    assert_eq!(
        outcome(&request(13)),
        committed(vec![
            Op::SetBlockData {
                pos: p(13),
                data: None,
            },
            tell("absent"),
        ])
    );
    assert_eq!(
        outcome(&request(14)),
        committed(vec![
            Op::SetBlockData {
                pos: p(14),
                data: Some(String::new()),
            },
            tell("empty"),
        ])
    );
}

#[test]
fn info_is_passed_through() {
    let mut request = interact(15);
    let Request::Callback { info, .. } = &mut request else {
        unreachable!("a callback request");
    };
    info.tick = 77;
    info.event_sequence = 5;
    assert_eq!(outcome(&request), committed(vec![tell("tick 77 seq 5")]));
}

#[test]
fn place_break_neighbor_reach_guest() {
    let change = |before_id: &str, after_id: &str, previous_data: Option<&str>| Change {
        pos: p(0),
        actor: Some(ACTOR.to_owned()),
        cause: Cause::Player,
        before_id: before_id.to_owned(),
        after_id: after_id.to_owned(),
        previous_data: previous_data.map(str::to_owned),
    };
    let place = callback(
        p(0),
        Call::Place {
            change: change(AIR, COUNTER, None),
        },
    );
    assert_eq!(outcome(&place), committed(vec![tell("placed")]));
    let broken = callback(
        p(0),
        Call::Break {
            change: change(COUNTER, AIR, Some("0a")),
        },
    );
    let broken = with_cell(broken, cell(p(0), AIR, false, None));
    assert_eq!(outcome(&broken), committed(vec![tell("broke 1")]));
    let neighbor = callback(
        p(0),
        Call::Neighbor {
            pos: p(0),
            neighbor: up(0),
        },
    );
    assert_eq!(outcome(&neighbor), committed(vec![]));
}

/// x=17 replaces the owned `up`, which holds data, with the same block, then reads its data.
#[test]
fn replacing_block_clears_its_data_in_overlay() {
    let request = with_cell(interact(17), cell(up(17), COUNTER, true, Some("0a")));
    assert_eq!(
        outcome(&request),
        committed(vec![
            Op::SetBlock {
                pos: up(17),
                id: COUNTER.to_owned(),
            },
            tell("absent"),
        ])
    );
}

/// Snapshot data that is not hex runs nothing: the guest would have committed.
#[test]
fn malformed_request_is_rejected_unrun() {
    let request = with_cell(interact(0), cell(p(0), COUNTER, true, Some("zz")));
    let outcome = outcome(&request);
    assert!(matches!(outcome, Outcome::Rejected { .. }), "{outcome:?}");
}

/// A player id must be a canonical lowercase hyphenated UUID wherever it appears, or nothing runs:
/// each of these callbacks would commit if it ran. An absent actor is fine.
#[test]
fn non_canonical_player_ids_are_rejected_unrun() {
    let shouted = ACTOR.to_uppercase();
    let place = |actor: Option<String>| {
        callback(
            p(0),
            Call::Place {
                change: Change {
                    pos: p(0),
                    actor,
                    cause: Cause::Player,
                    before_id: AIR.to_owned(),
                    after_id: COUNTER.to_owned(),
                    previous_data: None,
                },
            },
        )
    };
    let with_actor = |mut request: Request, id: Option<String>| {
        let Request::Callback { actor, .. } = &mut request else {
            unreachable!("a callback request");
        };
        *actor = id;
        request
    };
    let player = callback(
        p(0),
        Call::Interact {
            player: shouted.clone(),
            pos: p(0),
            face: Face::Up,
        },
    );
    let malformed = [
        with_actor(interact(0), Some(shouted.clone())),
        player,
        place(Some(shouted)),
    ];
    for request in malformed {
        let outcome = outcome(&request);
        assert!(matches!(outcome, Outcome::Rejected { .. }), "{outcome:?}");
    }
    assert_eq!(outcome(&with_actor(place(None), None)), committed(vec![]));
}

/// A staged client message commits with the result, in the order it was staged.
#[test]
fn staged_send_commits_with_the_result() {
    assert_eq!(
        outcome(&interact(19)),
        committed(vec![
            Op::SetBlockData {
                pos: p(19),
                data: Some("01000000".to_owned()),
            },
            send("probe.counter", 1, vec![Scalar::Integer(1)]),
            tell("count 1 ok"),
        ])
    );
}

#[test]
fn send_to_non_actor_is_denied() {
    assert_eq!(outcome(&interact(20)), committed(vec![tell("denied")]));
}

#[test]
fn oversized_send_is_too_large() {
    assert_eq!(outcome(&interact(21)), committed(vec![tell("too-large")]));
}

/// A client message reaches the guest with its fields in order. Its callback has no snapshot,
/// so the block read and write are refused, while the echo to the sender is staged.
#[test]
fn client_message_reaches_guest_without_world_access() {
    let payload = vec![
        Scalar::Bool(true),
        Scalar::Integer(-42),
        Scalar::Text("ack".to_owned()),
        Scalar::Choice(3),
    ];
    assert_eq!(
        outcome(&client_message("probe.echo", 7, payload.clone())),
        committed(vec![
            send("probe.echo", 7, payload),
            tell("client probe.echo 7 4 read denied write denied echo ok"),
        ])
    );
}

/// A client message comes from its player, who is the callback's actor, and carries no
/// snapshot; otherwise nothing runs, though each of these would commit if it ran.
#[test]
fn malformed_client_message_is_rejected_unrun() {
    let with = |edit: fn(&mut Option<String>, &mut Vec<Cell>)| {
        let mut request = client_message("probe.echo", 1, vec![]);
        let Request::Callback {
            actor, snapshot, ..
        } = &mut request
        else {
            unreachable!("a callback request");
        };
        edit(actor, snapshot);
        request
    };
    let malformed = [
        with(|actor, _| *actor = None),
        with(|actor, _| *actor = Some("00000000-0000-0000-0000-000000000000".to_owned())),
        with(|_, snapshot| snapshot.push(cell(p(0), COUNTER, true, None))),
    ];
    for request in malformed {
        let outcome = outcome(&request);
        assert!(matches!(outcome, Outcome::Rejected { .. }), "{outcome:?}");
    }
    assert!(matches!(
        outcome(&with(|_, _| {})),
        Outcome::Committed { .. }
    ));
}
