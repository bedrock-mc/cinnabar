//! The world-access rules of [`CallbackRes`], called directly on a prepared callback.

mod item_metadata;

use item_metadata::server_item;

use std::sync::Arc;

use wasmtime::Trap;

use super::{CallbackRes, failed, prepare};
use crate::host::LimitExceeded;
use crate::host::cinnabar::experience_server::types::{
    Scalar as Leaf, ValueNode as Node, WorldError,
};
use crate::limits::{
    MAX_BLOCK_DATA_BYTES, MAX_CLIENT_SEND_BYTES, MAX_CLIENT_SENDS, MAX_HOST_CALLS,
    MAX_ITEM_DATA_BYTES, MAX_NETWORK_BLOCKS, MAX_NETWORK_DATA_BYTES, MAX_STAGED_DATA_BYTES,
    MAX_STAGED_OPS, MAX_TELL_BYTES, MAX_TELLS, MAX_VALUE_DEPTH,
};
use crate::load::{Catalog, axes};
use crate::protocol::{
    BlockPos, BlockState, Call, Cell, Face, FailKind, INVENTORY_SLOTS, Info, Inventory, ItemStack,
    Network, NewStack, Op, Outcome, PlacementState, Request, Scalar, StateDef, StateValue,
    StateValues,
};

const ACTOR: &str = "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c";
const COUNTER: &str = "probe:counter";
/// A block with states: the facing trait's, then `probe:on` and `probe:colour`.
const LAMP: &str = "probe:lamp";
/// The Experience's item, one to a stack.
const CELL: &str = "probe:cell";
/// Server items, as the adapter lists them at load.
const STONE: &str = "minecraft:stone";
const PEARL: &str = "minecraft:ender_pearl";
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
    network: Option<Network>,
    inventory: Option<Inventory>,
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
            network: None,
            inventory: Some(Inventory {
                selected: 0,
                slots: vec![None; INVENTORY_SLOTS],
            }),
        }
    }

    /// Replaces the cell at `pos` with a loaded `id`; `data` is hex.
    fn cell(mut self, pos: BlockPos, id: &str, owned: bool, data: Option<&str>) -> Self {
        let cell = self.cells.iter_mut().find(|cell| cell.pos == pos);
        *cell.expect("a snapshot cell") = loaded(pos, id, owned, data);
        self
    }

    /// Gives the cell at `pos` these states.
    fn states(mut self, pos: BlockPos, states: Vec<BlockState>) -> Self {
        let cell = self.cells.iter_mut().find(|cell| cell.pos == pos);
        cell.expect("a snapshot cell").states = states;
        self
    }

    /// The callback's host value, for an Experience whose blocks are probe:counter, which has
    /// no states, and [`LAMP`].
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
            network: self.network,
            // The adapter snapshots an inventory only for an actor.
            inventory: self.actor.and(self.inventory),
            call: Call::Interact {
                player: ACTOR.to_owned(),
                pos: ANCHOR,
                face: Face::Up,
            },
        };
        let lamp = [
            StateDef {
                name: "probe:on".to_owned(),
                values: StateValues::Bool,
            },
            StateDef {
                name: "probe:colour".to_owned(),
                values: StateValues::Choices(vec!["red".to_owned(), "blue".to_owned()]),
            },
        ];
        let own = Catalog {
            blocks: [
                (COUNTER.to_owned(), Vec::new()),
                (
                    LAMP.to_owned(),
                    axes(&lamp, &[PlacementState::FacingDirection]),
                ),
            ]
            .into(),
            items: [(CELL.to_owned(), 1)].into(),
            server: [
                server_item(STONE, 0, 64, false),
                server_item(PEARL, 0, 16, false),
            ]
            .into(),
        };
        let (res, _) = prepare(&Arc::new(own), &request).unwrap();
        res
    }

    /// The request this fixture makes, prepared for an Experience of probe:counter alone, or why
    /// it is malformed.
    fn prepared(self) -> Result<CallbackRes, String> {
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
            network: self.network,
            inventory: self.inventory,
            call: Call::Interact {
                player: ACTOR.to_owned(),
                pos: ANCHOR,
                face: Face::Up,
            },
        };
        let own = Catalog {
            blocks: [(COUNTER.to_owned(), Vec::new())].into(),
            items: [(CELL.to_owned(), 1)].into(),
            server: [server_item(STONE, 0, 64, false)].into(),
        };
        prepare(&Arc::new(own), &request).map(|(res, _)| res)
    }

    /// Adds loaded cells of `id` at `positions` beyond the anchor's neighbors, owned, with
    /// `data` (hex).
    fn far(mut self, id: &str, data: Option<&str>, positions: &[BlockPos]) -> Self {
        for &pos in positions {
            self.cells.push(loaded(pos, id, true, data));
        }
        self
    }

    /// Gives the callback a network of `blocks`.
    fn network(mut self, blocks: &[BlockPos], truncated: bool) -> Self {
        self.network = Some(Network {
            blocks: blocks.to_vec(),
            truncated,
        });
        self
    }
}

fn loaded(pos: BlockPos, id: &str, owned: bool, data: Option<&str>) -> Cell {
    Cell {
        pos,
        loaded: true,
        id: id.to_owned(),
        owned,
        data: data.map(str::to_owned),
        states: Vec::new(),
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

fn state(name: &str, value: StateValue) -> BlockState {
    BlockState {
        name: name.to_owned(),
        value,
    }
}

fn on(value: bool) -> BlockState {
    state("probe:on", StateValue::Bool(value))
}

fn colour(value: &str) -> BlockState {
    state("probe:colour", StateValue::Choice(value.to_owned()))
}

fn facing(value: &str) -> BlockState {
    state(
        "minecraft:facing_direction",
        StateValue::Choice(value.to_owned()),
    )
}

/// A lamp's states as the adapter snapshots them: every axis, the trait's first.
fn lamp(face: &str, lit: bool, hue: &str) -> Vec<BlockState> {
    vec![facing(face), on(lit), colour(hue)]
}

/// [`Fixture::new`] with a lamp at the anchor in place of the counter, with data `01`.
fn lamp_fixture() -> Fixture {
    Fixture::new()
        .cell(ANCHOR, LAMP, true, Some("01"))
        .states(ANCHOR, lamp("north", false, "red"))
}

/// States are read like ids: an own block's as snapshotted, nothing for other blocks.
#[test]
fn block_states_read_the_snapshot() {
    let mut res = lamp_fixture().cell(UP, COUNTER, true, None).res();
    assert_eq!(
        res.block_states(ANCHOR).unwrap(),
        Ok(lamp("north", false, "red"))
    );
    assert_eq!(res.block_states(UP).unwrap(), Ok(Vec::new()));
    assert_eq!(res.block_states(EAST).unwrap(), Ok(Vec::new()));
    assert_eq!(
        res.block_states(BlockPos { x: 5, ..ANCHOR }).unwrap(),
        Err(WorldError::Denied)
    );
}

/// Writes to one block merge into one op holding each state's last value, and the block keeps
/// its data.
#[test]
fn set_block_state_merges_and_keeps_data() {
    let mut res = lamp_fixture().res();
    assert_eq!(res.set_block_state(ANCHOR, vec![on(true)]).unwrap(), Ok(()));
    assert_eq!(
        res.set_block_state(ANCHOR, vec![colour("blue"), facing("up")])
            .unwrap(),
        Ok(())
    );
    assert_eq!(
        res.set_block_state(ANCHOR, vec![on(false)]).unwrap(),
        Ok(())
    );
    assert_eq!(
        res.ops,
        vec![Op::SetBlockState {
            pos: ANCHOR,
            states: vec![on(false), colour("blue"), facing("up")],
        }]
    );
    assert_eq!(
        res.block_states(ANCHOR).unwrap(),
        Ok(lamp("up", false, "blue"))
    );
    assert_eq!(res.block_data(ANCHOR).unwrap(), Ok(Some(vec![1])));
}

/// A state the block lacks, a value its state does not take, or one state twice is refused
/// whole, staging nothing; so is any state of a block that has none. An empty write stages
/// nothing either.
#[test]
fn set_block_state_refuses_unknown_states_and_values() {
    let mut res = lamp_fixture().cell(UP, COUNTER, true, None).res();
    for states in [
        vec![state("probe:dim", StateValue::Bool(true))],
        vec![state("probe:on", StateValue::Choice("yes".to_owned()))],
        vec![colour("green")],
        vec![
            colour("blue"),
            state("probe:colour", StateValue::Bool(true)),
        ],
        vec![on(true), on(false)],
        vec![facing("sideways")],
    ] {
        assert_eq!(
            res.set_block_state(ANCHOR, states.clone()).unwrap(),
            Err(WorldError::UnsupportedState),
            "{states:?}"
        );
    }
    assert_eq!(
        res.set_block_state(UP, vec![on(true)]).unwrap(),
        Err(WorldError::UnsupportedState)
    );
    assert_eq!(res.set_block_state(UP, Vec::new()).unwrap(), Ok(()));
    assert_eq!(res.ops, Vec::new());
    assert_eq!(
        res.block_states(ANCHOR).unwrap(),
        Ok(lamp("north", false, "red"))
    );
}

/// States are written where data is: own blocks in the anchor's chunk column.
#[test]
fn set_block_state_only_reaches_own_blocks_in_scope() {
    let mut res = lamp_fixture()
        .cell(WEST, LAMP, true, None)
        .cell(EAST, LAMP, false, None)
        .cell(UP, "minecraft:stone", false, None)
        .res();
    assert_eq!(
        res.set_block_state(WEST, vec![on(true)]).unwrap(),
        Err(WorldError::Denied)
    );
    for pos in [EAST, UP, DOWN] {
        assert_eq!(
            res.set_block_state(pos, vec![on(true)]).unwrap(),
            Err(WorldError::NotOwned),
            "{pos:?}"
        );
    }
    assert_eq!(res.ops, Vec::new());
}

/// A replacement resets the block's states to its defaults, the first value of each, and drops
/// the states staged before it; states staged after it stay after it.
#[test]
fn replacement_resets_states() {
    let mut res = lamp_fixture().res();
    assert_eq!(res.set_block_state(ANCHOR, vec![on(true)]).unwrap(), Ok(()));
    assert_eq!(res.set_block(ANCHOR, LAMP.to_owned()).unwrap(), Ok(()));
    assert_eq!(
        res.block_states(ANCHOR).unwrap(),
        Ok(lamp("down", false, "red"))
    );
    assert_eq!(
        res.set_block_state(ANCHOR, vec![colour("blue")]).unwrap(),
        Ok(())
    );
    assert_eq!(
        res.ops,
        vec![
            Op::SetBlock {
                pos: ANCHOR,
                id: LAMP.to_owned(),
            },
            Op::SetBlockState {
                pos: ANCHOR,
                states: vec![colour("blue")],
            },
        ]
    );
    assert_eq!(res.set_block(ANCHOR, AIR.to_owned()).unwrap(), Ok(()));
    assert_eq!(res.block_states(ANCHOR).unwrap(), Ok(Vec::new()));
    assert_eq!(
        res.set_block_state(ANCHOR, vec![on(true)]).unwrap(),
        Err(WorldError::NotOwned)
    );
}

/// A member of the anchor's network two chunk columns east, and one north of it.
const FAR: BlockPos = BlockPos { x: 40, y: 64, z: 0 };
const FAR_NORTH: BlockPos = BlockPos {
    x: 40,
    y: 64,
    z: -1,
};

/// Without a network, `network` is none; with one, it is the members and whether a bound cut
/// them short, as the adapter snapshotted them.
#[test]
fn network_reports_the_snapshotted_members() {
    assert_eq!(Fixture::new().res().network().unwrap(), Ok(None));
    let mut res = Fixture::new()
        .far(LAMP, None, &[FAR])
        .network(&[ANCHOR, FAR], true)
        .res();
    assert_eq!(
        res.network().unwrap(),
        Ok(Some(Network {
            blocks: vec![ANCHOR, FAR],
            truncated: true,
        }))
    );
}

/// Network members take data and states wherever they are, and are read like the anchor's
/// neighbors; blocks are still set only in the anchor's chunk column, and a cell outside the
/// network keeps the column's scope.
#[test]
fn network_members_take_data_and_states_but_not_blocks() {
    let mut res = Fixture::new()
        .cell(WEST, COUNTER, true, None)
        .far(LAMP, Some("01"), &[FAR])
        .far(COUNTER, None, &[FAR_NORTH])
        .states(FAR, lamp("north", false, "red"))
        .network(&[ANCHOR, FAR, FAR_NORTH], false)
        .res();
    assert_eq!(res.get_block(FAR).unwrap(), Ok(LAMP.to_owned()));
    assert_eq!(res.block_data(FAR).unwrap(), Ok(Some(vec![1])));
    assert_eq!(res.set_block_data(FAR, Some(vec![2])).unwrap(), Ok(()));
    assert_eq!(res.set_block_state(FAR, vec![on(true)]).unwrap(), Ok(()));
    assert_eq!(res.set_block_data(FAR_NORTH, None).unwrap(), Ok(()));
    assert_eq!(
        res.set_block(FAR, AIR.to_owned()).unwrap(),
        Err(WorldError::Denied)
    );
    assert_eq!(
        res.set_block_data(WEST, Some(vec![3])).unwrap(),
        Err(WorldError::Denied)
    );
    assert_eq!(
        res.ops,
        vec![
            Op::SetBlockData {
                pos: FAR,
                data: Some("02".to_owned()),
            },
            Op::SetBlockState {
                pos: FAR,
                states: vec![on(true)],
            },
            Op::SetBlockData {
                pos: FAR_NORTH,
                data: None,
            },
        ]
    );
}

/// A network the adapter could not have built is malformed: a member outside the snapshot, an
/// unloaded or foreign one, more than MAX_NETWORK_BLOCKS members, or more than
/// MAX_NETWORK_DATA_BYTES of their data.
#[test]
fn malformed_networks_are_refused() {
    let at = |i: usize| BlockPos {
        x: 100 + i as i32,
        y: 64,
        z: 0,
    };
    let members = |count: usize| (0..count).map(at).collect::<Vec<_>>();
    let chunk = "00".repeat(MAX_BLOCK_DATA_BYTES);
    let full = MAX_NETWORK_DATA_BYTES / MAX_BLOCK_DATA_BYTES;
    let accepted = [
        Fixture::new()
            .far(COUNTER, None, &members(MAX_NETWORK_BLOCKS))
            .network(&members(MAX_NETWORK_BLOCKS), true),
        Fixture::new()
            .far(COUNTER, Some(&chunk), &members(full))
            .network(&members(full), false),
    ];
    for fixture in accepted {
        assert!(fixture.prepared().is_ok());
    }
    let mut unloaded = Fixture::new().network(&[ANCHOR, FAR], false);
    unloaded.cells.push(Cell {
        loaded: false,
        ..loaded(FAR, "", false, None)
    });
    let refused = [
        (Fixture::new().network(&[FAR], false), "not in the snapshot"),
        (unloaded, "not a loaded block of its own"),
        (
            Fixture::new()
                .cell(EAST, "minecraft:stone", false, None)
                .network(&[EAST], false),
            "not a loaded block of its own",
        ),
        (
            Fixture::new()
                .far(COUNTER, None, &members(MAX_NETWORK_BLOCKS + 1))
                .network(&members(MAX_NETWORK_BLOCKS + 1), true),
            "members",
        ),
        (
            Fixture::new()
                .far(COUNTER, Some(&chunk), &members(full + 1))
                .network(&members(full + 1), true),
            "bytes of data",
        ),
    ];
    for (fixture, cause) in refused {
        match fixture.prepared() {
            Ok(_) => panic!("accepted, not {cause:?}"),
            Err(error) => assert!(error.contains(cause), "{error:?} lacks {cause:?}"),
        }
    }
}

/// A stack the guest makes: `count` of `id` with `data` (bytes).
fn new_stack(id: &str, count: u8, data: Option<&[u8]>) -> NewStack {
    NewStack {
        id: id.to_owned(),
        metadata: 0,
        count,
        data: data.map(crate::hex::encode),
    }
}

/// A plain snapshot stack of `count` of `id`, `max` to a stack.
fn stack(id: &str, count: u8, max: u8) -> ItemStack {
    ItemStack {
        id: id.to_owned(),
        metadata: 0,
        count,
        max_count: max,
        data: None,
        plain: true,
    }
}

impl Fixture {
    /// Puts `stack` in the actor's inventory slot `slot`.
    fn slot(mut self, slot: usize, stack: Option<ItemStack>) -> Self {
        self.inventory.as_mut().expect("an inventory").slots[slot] = stack;
        self
    }
}

/// `inventory` is the snapshot with the staged slots applied, each staged stack plain with the
/// most one stack holds; writing a slot again replaces its op, and none empties it.
#[test]
fn inventory_reads_the_snapshot_with_staged_slots() {
    let mut res = Fixture::new()
        .slot(0, Some(stack(STONE, 32, 64)))
        .slot(36, Some(stack(CELL, 1, 1)))
        .res();
    let read = res.inventory().unwrap().unwrap();
    assert_eq!(read.selected, 0);
    assert_eq!(read.slots[0], Some(stack(STONE, 32, 64)));
    assert_eq!(read.slots.len(), INVENTORY_SLOTS);
    let cell = new_stack(CELL, 1, Some(&[1, 2]));
    assert_eq!(
        res.set_slot(1, Some(new_stack(STONE, 1, None))).unwrap(),
        Ok(())
    );
    assert_eq!(res.set_slot(1, Some(cell.clone())).unwrap(), Ok(()));
    assert_eq!(res.set_slot(0, None).unwrap(), Ok(()));
    let read = res.inventory().unwrap().unwrap();
    assert_eq!(read.slots[0], None);
    assert_eq!(
        read.slots[1],
        Some(ItemStack {
            data: Some("0102".to_owned()),
            ..stack(CELL, 1, 1)
        })
    );
    assert_eq!(
        res.ops,
        vec![
            Op::SetSlot {
                slot: 1,
                stack: Some(cell),
            },
            Op::SetSlot {
                slot: 0,
                stack: None,
            },
        ]
    );
}

/// Stack writes enforce catalogue limits and ownership; refused writes stage nothing.
#[test]
fn set_slot_refuses_what_the_guest_cannot_make() {
    let mut res = Fixture::new().res();
    let big = vec![0; MAX_ITEM_DATA_BYTES + 1];
    let cases = [
        (37, Some(new_stack(STONE, 1, None)), WorldError::OutOfBounds),
        (
            0,
            Some(new_stack("minecraft:nonsense", 1, None)),
            WorldError::UnknownBlock,
        ),
        (
            0,
            Some(new_stack("other:cell", 1, None)),
            WorldError::UnknownBlock,
        ),
        (0, Some(new_stack(STONE, 65, None)), WorldError::TooLarge),
        (0, Some(new_stack(PEARL, 17, None)), WorldError::TooLarge),
        (0, Some(new_stack(CELL, 2, None)), WorldError::TooLarge),
        (0, Some(new_stack(COUNTER, 65, None)), WorldError::TooLarge),
        (
            0,
            Some(new_stack(STONE, 1, Some(&[1]))),
            WorldError::NotOwned,
        ),
        (
            0,
            Some(new_stack(COUNTER, 1, Some(&[1]))),
            WorldError::NotOwned,
        ),
        (
            0,
            Some(new_stack(CELL, 1, Some(&big))),
            WorldError::TooLarge,
        ),
        (
            0,
            Some(new_stack(STONE, 0, None)),
            WorldError::UnsupportedState,
        ),
        (
            0,
            Some(NewStack {
                metadata: 1,
                ..new_stack(CELL, 1, None)
            }),
            WorldError::UnsupportedState,
        ),
    ];
    for (slot, stack, error) in cases {
        assert_eq!(
            res.set_slot(slot, stack.clone()).unwrap(),
            Err(error),
            "{slot} {stack:?}"
        );
    }
    assert_eq!(res.ops, Vec::new());
    for ok in [
        new_stack(PEARL, 16, None),
        new_stack(COUNTER, 64, None),
        new_stack(CELL, 1, Some(&vec![0; MAX_ITEM_DATA_BYTES])),
    ] {
        assert_eq!(res.set_slot(0, Some(ok.clone())).unwrap(), Ok(()), "{ok:?}");
    }
}

/// Without an actor there is no inventory to read or write.
#[test]
fn inventory_needs_an_actor() {
    let mut res = Fixture {
        actor: None,
        inventory: None,
        ..Fixture::new()
    }
    .res();
    assert_eq!(res.inventory().unwrap(), Err(WorldError::PlayerUnavailable));
    assert_eq!(
        res.set_slot(0, None).unwrap(),
        Err(WorldError::PlayerUnavailable)
    );
}

/// An item drops where a block may be set, from a stack the guest may make.
#[test]
fn drop_item_stays_in_the_column() {
    let mut res = Fixture::new().res();
    let stone = new_stack(STONE, 3, None);
    assert_eq!(res.drop_item(UP, stone.clone()).unwrap(), Ok(()));
    assert_eq!(
        res.drop_item(WEST, stone.clone()).unwrap(),
        Err(WorldError::Denied)
    );
    assert_eq!(
        res.drop_item(BlockPos { x: 5, ..ANCHOR }, stone.clone())
            .unwrap(),
        Err(WorldError::Denied)
    );
    assert_eq!(
        res.drop_item(UP, new_stack("minecraft:nonsense", 1, None))
            .unwrap(),
        Err(WorldError::UnknownBlock)
    );
    assert_eq!(
        res.ops,
        vec![Op::DropItem {
            pos: UP,
            stack: stone,
        }]
    );
}

/// An inventory the adapter could not have built is malformed: not 37 slots, a selected slot
/// outside the hotbar, a stack past its most, data on a stack not the Experience's own, data
/// past MAX_ITEM_DATA_BYTES, or an inventory without an actor.
#[test]
fn malformed_inventories_are_refused() {
    let big = "00".repeat(MAX_ITEM_DATA_BYTES + 1);
    let with = |edit: fn(&mut Inventory)| {
        let mut fixture = Fixture::new();
        edit(fixture.inventory.as_mut().unwrap());
        fixture
    };
    let mut no_actor = Fixture::new();
    no_actor.actor = None;
    let refused = [
        (
            with(|i| {
                i.slots.pop();
            }),
            "slots",
        ),
        (with(|i| i.selected = 9), "selected"),
        (
            with(|i| i.slots[0] = Some(stack(STONE, 65, 64))),
            "more than",
        ),
        (
            with(|i| {
                i.slots[0] = Some(ItemStack {
                    data: Some("01".to_owned()),
                    ..stack(STONE, 1, 64)
                })
            }),
            "data",
        ),
        (
            with(|i| {
                i.slots[0] = Some(ItemStack {
                    data: Some("zz".to_owned()),
                    ..stack(CELL, 1, 1)
                })
            }),
            "hex",
        ),
        (no_actor, "without an actor"),
    ];
    for (fixture, cause) in refused {
        match fixture.prepared() {
            Ok(_) => panic!("accepted, not {cause:?}"),
            Err(error) => assert!(error.contains(cause), "{error:?} lacks {cause:?}"),
        }
    }
    let mut over = Fixture::new();
    over.inventory.as_mut().unwrap().slots[0] = Some(ItemStack {
        data: Some(big),
        ..stack(CELL, 1, 1)
    });
    match over.prepared() {
        Ok(_) => panic!("accepted item data past the limit"),
        Err(error) => assert!(error.contains("bytes"), "{error}"),
    }
}
