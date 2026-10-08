//! The world-access rules of [`CallbackRes`], called directly on a prepared callback.

use std::sync::Arc;

use wasmtime::Trap;

use super::{CallbackRes, failed, prepare};
use crate::host::LimitExceeded;
use crate::host::cinnabar::experience_server::types::{
    Scalar as Leaf, ValueNode as Node, WorldError,
};
use crate::limits::{
    MAX_BLOCK_DATA_BYTES, MAX_CLIENT_SEND_BYTES, MAX_CLIENT_SENDS, MAX_HOST_CALLS,
    MAX_STAGED_DATA_BYTES, MAX_STAGED_OPS, MAX_TELL_BYTES, MAX_TELLS, MAX_VALUE_DEPTH,
};
use crate::protocol::{BlockPos, Call, Cell, Face, FailKind, Info, Op, Outcome, Request, Scalar};

const ACTOR: &str = "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c";
const COUNTER: &str = "probe:counter";
const AIR: &str = "minecraft:air";

const ANCHOR: BlockPos = BlockPos { x: 0, y: 64, z: 0 };
const UP: BlockPos = BlockPos { x: 0, y: 65, z: 0 };
const DOWN: BlockPos = BlockPos { x: 0, y: 63, z: 0 };
const EAST: BlockPos = BlockPos { x: 1, y: 64, z: 0 };
const WEST: BlockPos = BlockPos { x: -1, y: 64, z: 0 };
const NORTH: BlockPos = BlockPos { x: 0, y: 64, z: -1 };
const SOUTH: BlockPos = BlockPos { x: 0, y: 64, z: 1 };

/// The actor's interaction with [`ANCHOR`], an owned probe:counter without data whose six
/// neighbors are loaded air, with room to spare in the world and the data budget.
struct Fixture {
    actor: Option<&'static str>,
    min_y: i32,
    max_y: i32,
    budget: u64,
    cells: Vec<Cell>,
}

impl Fixture {
    fn new() -> Self {
        let mut cells = vec![loaded(ANCHOR, COUNTER, true, None)];
        for pos in [UP, DOWN, EAST, WEST, NORTH, SOUTH] {
            cells.push(loaded(pos, AIR, false, None));
        }
        Self {
            actor: Some(ACTOR),
            min_y: -64,
            max_y: 319,
            budget: 1 << 20,
            cells,
        }
    }

    /// Replaces the cell at `pos` with a loaded `id`; `data` is hex.
    fn cell(mut self, pos: BlockPos, id: &str, owned: bool, data: Option<&str>) -> Self {
        let cell = self.cells.iter_mut().find(|cell| cell.pos == pos);
        *cell.expect("a snapshot cell") = loaded(pos, id, owned, data);
        self
    }

    /// The callback's host value, for an Experience whose only block is probe:counter.
    fn res(self) -> CallbackRes {
        let request = Request::Callback {
            seq: 1,
            info: Info {
                world_id: "world".to_owned(),
                dimension_id: "overworld".to_owned(),
                tick: 1,
                event_sequence: 1,
            },
            actor: self.actor.map(str::to_owned),
            world_min_y: self.min_y,
            world_max_y: self.max_y,
            data_budget: self.budget,
            snapshot: self.cells,
            call: Call::Interact {
                player: ACTOR.to_owned(),
                pos: ANCHOR,
                face: Face::Up,
            },
        };
        let (res, _) = prepare(&Arc::from([COUNTER.to_owned()]), &request).unwrap();
        res
    }
}

fn loaded(pos: BlockPos, id: &str, owned: bool, data: Option<&str>) -> Cell {
    Cell {
        pos,
        loaded: true,
        id: id.to_owned(),
        owned,
        data: data.map(str::to_owned),
    }
}

/// Whether `result` is a trap that fails the callback as `limit`.
fn limited<T>(result: anyhow::Result<T>) -> bool {
    result.is_err_and(|error| error.is::<LimitExceeded>())
}

/// Both bounds are inside the world; the cells past them are outside even though the
/// snapshot holds them.
#[test]
fn world_height_bounds_reads_and_writes() {
    let mut res = Fixture {
        min_y: ANCHOR.y,
        max_y: ANCHOR.y,
        ..Fixture::new()
    }
    .res();
    assert_eq!(res.get_block(ANCHOR).unwrap(), Ok(COUNTER.to_owned()));
    assert_eq!(res.get_block(UP).unwrap(), Err(WorldError::OutOfBounds));
    assert_eq!(res.block_data(DOWN).unwrap(), Err(WorldError::OutOfBounds));
    assert_eq!(
        res.set_block(UP, AIR.to_owned()).unwrap(),
        Err(WorldError::OutOfBounds)
    );
    assert_eq!(
        res.set_block_data(DOWN, None).unwrap(),
        Err(WorldError::OutOfBounds)
    );
}

/// West and north of the anchor lie in other chunk columns (`-1 >> 4 == -1`); east shares
/// the anchor's. Reads reach all of them.
#[test]
fn writes_stay_in_the_anchor_chunk_column() {
    let mut res = Fixture::new().cell(NORTH, COUNTER, true, None).res();
    assert_eq!(res.get_block(WEST).unwrap(), Ok(AIR.to_owned()));
    assert_eq!(
        res.set_block(WEST, COUNTER.to_owned()).unwrap(),
        Err(WorldError::Denied)
    );
    assert_eq!(res.block_data(NORTH).unwrap(), Ok(None));
    assert_eq!(
        res.set_block_data(NORTH, Some(vec![1])).unwrap(),
        Err(WorldError::Denied)
    );
    assert_eq!(res.set_block(EAST, COUNTER.to_owned()).unwrap(), Ok(()));
}

/// Placing makes air an owned block without data; removing makes it air that is not owned.
/// Refused replacements stage nothing.
#[test]
fn set_block_replaces_air_or_own_blocks_with_known_ids() {
    let mut res = Fixture::new()
        .cell(EAST, "minecraft:stone", false, None)
        .res();
    assert_eq!(
        res.set_block(EAST, AIR.to_owned()).unwrap(),
        Err(WorldError::NotOwned)
    );
    assert_eq!(
        res.set_block(UP, "probe:missing".to_owned()).unwrap(),
        Err(WorldError::UnknownBlock)
    );
    assert_eq!(res.set_block(UP, COUNTER.to_owned()).unwrap(), Ok(()));
    assert_eq!(res.block_data(UP).unwrap(), Ok(None));
    assert_eq!(res.set_block(ANCHOR, AIR.to_owned()).unwrap(), Ok(()));
    assert_eq!(res.block_data(ANCHOR).unwrap(), Err(WorldError::NotOwned));
    assert_eq!(
        res.ops,
        vec![
            Op::SetBlock {
                pos: UP,
                id: COUNTER.to_owned(),
            },
            Op::SetBlock {
                pos: ANCHOR,
                id: AIR.to_owned(),
            },
        ]
    );
}

#[test]
fn data_belongs_to_own_blocks_only() {
    let mut res = Fixture::new().res();
    assert_eq!(res.block_data(UP).unwrap(), Err(WorldError::NotOwned));
    assert_eq!(
        res.set_block_data(UP, Some(vec![1])).unwrap(),
        Err(WorldError::NotOwned)
    );
}

/// The limit itself fits; one byte more is refused and leaves the data as it was.
#[test]
fn block_data_limit_is_inclusive() {
    let mut res = Fixture::new().res();
    let full = vec![7; MAX_BLOCK_DATA_BYTES];
    assert_eq!(
        res.set_block_data(ANCHOR, Some(full.clone())).unwrap(),
        Ok(())
    );
    assert_eq!(
        res.set_block_data(ANCHOR, Some(vec![0; MAX_BLOCK_DATA_BYTES + 1]))
            .unwrap(),
        Err(WorldError::TooLarge)
    );
    assert_eq!(res.block_data(ANCHOR).unwrap(), Ok(Some(full)));
}

/// The budget bounds the bytes a callback adds in total, so shrinking or clearing data
/// frees room for later writes.
#[test]
fn quota_counts_net_growth() {
    let mut res = Fixture {
        budget: 3,
        ..Fixture::new()
    }
    .cell(ANCHOR, COUNTER, true, Some("0102"))
    .cell(EAST, COUNTER, true, None)
    .res();
    // Growing 2 bytes to 5 adds exactly the budget.
    assert_eq!(
        res.set_block_data(ANCHOR, Some(vec![0; 5])).unwrap(),
        Ok(())
    );
    assert_eq!(
        res.set_block_data(EAST, Some(vec![0])).unwrap(),
        Err(WorldError::QuotaExceeded)
    );
    // Replacing the anchor clears its 5 bytes.
    assert_eq!(res.set_block(ANCHOR, COUNTER.to_owned()).unwrap(), Ok(()));
    assert_eq!(res.set_block_data(EAST, Some(vec![0; 5])).unwrap(), Ok(()));
    assert_eq!(
        res.set_block_data(EAST, Some(vec![0; 6])).unwrap(),
        Err(WorldError::QuotaExceeded)
    );
}

/// A rewrite replaces the op it rewrites, so it neither grows the result nor counts against
/// the op cap, and the last write is the one staged.
#[test]
fn rewriting_data_stages_one_op() {
    let mut res = Fixture::new().res();
    for _ in 0..MAX_STAGED_OPS {
        assert_eq!(res.set_block_data(ANCHOR, Some(vec![0])).unwrap(), Ok(()));
    }
    assert_eq!(
        res.set_block_data(ANCHOR, Some(vec![0xab])).unwrap(),
        Ok(())
    );
    assert_eq!(
        res.ops,
        vec![Op::SetBlockData {
            pos: ANCHOR,
            data: Some("ab".to_owned()),
        }]
    );
}

/// The replacement clears the data staged before it, so that op is dropped. The ops are
/// applied in order, so data staged after the replacement must stay after it.
#[test]
fn replacement_drops_data_staged_before_it() {
    let mut res = Fixture::new().res();
    assert_eq!(res.set_block_data(ANCHOR, Some(vec![1])).unwrap(), Ok(()));
    assert_eq!(res.set_block(ANCHOR, COUNTER.to_owned()).unwrap(), Ok(()));
    assert_eq!(res.set_block_data(ANCHOR, Some(vec![2])).unwrap(), Ok(()));
    assert_eq!(
        res.ops,
        vec![
            Op::SetBlock {
                pos: ANCHOR,
                id: COUNTER.to_owned(),
            },
            Op::SetBlockData {
                pos: ANCHOR,
                data: Some("02".to_owned()),
            },
        ]
    );
}

/// Owned cells in the anchor's chunk column whose full data fills the staged-data limit.
const FULL: [BlockPos; 4] = [ANCHOR, UP, DOWN, EAST];

/// A callback that has staged [`MAX_BLOCK_DATA_BYTES`] in every cell of [`FULL`], which
/// reaches the staged-data limit exactly. [`SOUTH`] is owned too and has no data.
fn full_staged_data() -> CallbackRes {
    assert_eq!(
        FULL.len() * MAX_BLOCK_DATA_BYTES,
        MAX_STAGED_DATA_BYTES,
        "FULL must fill the staged-data limit exactly"
    );
    let mut res = Fixture::new()
        .cell(UP, COUNTER, true, None)
        .cell(DOWN, COUNTER, true, None)
        .cell(EAST, COUNTER, true, None)
        .cell(SOUTH, COUNTER, true, None)
        .res();
    for pos in FULL {
        let full = Some(vec![0; MAX_BLOCK_DATA_BYTES]);
        assert_eq!(res.set_block_data(pos, full).unwrap(), Ok(()));
    }
    res
}

/// The limit itself fits; a byte more is refused and stages nothing.
#[test]
fn staged_data_limit_is_inclusive() {
    let mut res = full_staged_data();
    assert_eq!(
        res.set_block_data(SOUTH, Some(vec![0])).unwrap(),
        Err(WorldError::TooLarge)
    );
    assert_eq!(res.block_data(SOUTH).unwrap(), Ok(None));
    assert_eq!(res.ops.len(), FULL.len());
}

/// Only the ops left staged count, so shrinking a rewrite and replacing a block both free
/// room.
#[test]
fn staged_data_counts_the_ops_left_after_rewrites() {
    let mut res = full_staged_data();
    let shrunk = Some(vec![0; MAX_BLOCK_DATA_BYTES - 1]);
    assert_eq!(res.set_block_data(ANCHOR, shrunk).unwrap(), Ok(()));
    assert_eq!(res.set_block_data(SOUTH, Some(vec![0])).unwrap(), Ok(()));
    assert_eq!(res.set_block(UP, COUNTER.to_owned()).unwrap(), Ok(()));
    let full = Some(vec![0; MAX_BLOCK_DATA_BYTES]);
    assert_eq!(res.set_block_data(SOUTH, full).unwrap(), Ok(()));
}

#[test]
fn tell_reaches_only_the_actor() {
    let mut res = Fixture::new().res();
    let stranger = "00000000-0000-0000-0000-000000000000".to_owned();
    assert_eq!(
        res.tell(stranger, "x".to_owned()).unwrap(),
        Err(WorldError::Denied)
    );
    let mut res = Fixture {
        actor: None,
        ..Fixture::new()
    }
    .res();
    assert_eq!(
        res.tell(ACTOR.to_owned(), "x".to_owned()).unwrap(),
        Err(WorldError::PlayerUnavailable)
    );
}

/// The size limit counts UTF-8 bytes, so 129 two-byte characters are too many.
#[test]
fn tell_text_is_plain_and_short() {
    let mut res = Fixture::new().res();
    let mut tell = |text: String| res.tell(ACTOR.to_owned(), text).unwrap();
    assert_eq!(tell("a\nb".to_owned()), Err(WorldError::InvalidText));
    assert_eq!(tell("§cred".to_owned()), Err(WorldError::InvalidText));
    assert_eq!(
        tell("a".repeat(MAX_TELL_BYTES + 1)),
        Err(WorldError::TooLarge)
    );
    assert_eq!(
        tell("é".repeat(MAX_TELL_BYTES / 2 + 1)),
        Err(WorldError::TooLarge)
    );
    assert_eq!(tell("a".repeat(MAX_TELL_BYTES)), Ok(()));
    assert_eq!(
        res.ops,
        vec![Op::Tell {
            player: ACTOR.to_owned(),
            text: "a".repeat(MAX_TELL_BYTES),
        }]
    );
}

#[test]
fn tell_past_the_cap_traps() {
    let mut res = Fixture::new().res();
    for _ in 0..MAX_TELLS {
        assert_eq!(res.tell(ACTOR.to_owned(), "x".to_owned()).unwrap(), Ok(()));
    }
    assert!(limited(res.tell(ACTOR.to_owned(), "x".to_owned())));
}

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
            player: ACTOR.to_owned(),
            channel: "c".to_owned(),
            schema: 3,
            payload: tree,
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

#[test]
fn op_past_the_cap_traps() {
    let mut res = Fixture::new().res();
    for _ in 0..MAX_STAGED_OPS {
        assert_eq!(res.set_block(UP, AIR.to_owned()).unwrap(), Ok(()));
    }
    assert!(limited(res.set_block(UP, AIR.to_owned())));
}

/// `info` is a host call too.
#[test]
fn host_call_past_the_cap_traps() {
    let mut res = Fixture::new().res();
    for _ in 0..MAX_HOST_CALLS {
        assert_eq!(res.get_block(ANCHOR).unwrap(), Ok(COUNTER.to_owned()));
    }
    assert!(limited(res.info()));
}

/// A trap is classified and reported by its root cause, whatever context wraps it.
#[test]
fn failures_are_classified_by_cause() {
    let cases: [(anyhow::Error, FailKind); 4] = [
        (Trap::OutOfFuel.into(), FailKind::Fuel),
        (Trap::Interrupt.into(), FailKind::Deadline),
        (LimitExceeded("too many".to_owned()).into(), FailKind::Limit),
        (Trap::UnreachableCodeReached.into(), FailKind::Trap),
    ];
    for (cause, kind) in cases {
        let reason = cause.to_string();
        let error = cause.context("error while executing at wasm backtrace: …");
        assert_eq!(failed(&error), Outcome::Failed { kind, reason });
    }
}
