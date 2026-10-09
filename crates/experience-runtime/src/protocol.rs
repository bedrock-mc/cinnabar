//! The adapter protocol: message types, framing and the golden fixtures the Go adapter checks.
//!
//! A frame is a 4-byte little-endian length followed by that many bytes of JSON. Bytes inside
//! messages are lowercase hex (see [`crate::hex`]).

use std::io::{self, ErrorKind, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::hex;
use crate::limits::{
    MAX_BLOCK_DATA_BYTES, MAX_BONES, MAX_CLIENT_SEND_BYTES, MAX_CLIENT_SENDS, MAX_CONDITION_TESTS,
    MAX_FLIPBOOK_FRAMES, MAX_FRAME_BYTES, MAX_GEOMETRY_BYTES, MAX_ITEM_DATA_BYTES, MAX_ITEMS,
    MAX_MATERIALS, MAX_NAME_BYTES, MAX_NETWORK_BLOCKS, MAX_NETWORK_DATA_BYTES, MAX_PERMUTATIONS,
    MAX_REASON_BYTES, MAX_SERVER_ITEMS, MAX_STACK_SIZE, MAX_STAGED_OPS, MAX_STATE_COMBINATIONS,
    MAX_STATE_VALUES, MAX_TELL_BYTES, MAX_TELLS, MAX_VALUE_DEPTH,
};

mod player;
mod visuals;

pub use player::{
    HOTBAR_SLOTS, INVENTORY_SLOTS, Inventory, ItemStack, Network, NewStack, ServerItem,
};
pub use visuals::{
    BlockState, BoneVisibility, Condition, Flipbook, ItemDef, Material, Permutation, Pixel,
    PixelBox, PlacementState, QuarterTurns, RenderMethod, StateDef, StateTest, StateValue,
    StateValues, Visual,
};

/// 2 added the `client_message` call and the `send_client` op; 3 the `epoch` call and list and
/// record values; 4 the `focus` of those calls and of `loaded`; 5 server WIT 0.5's block states
/// and visuals, items, the actor's inventory, the network scope and their ops.
pub const PROTOCOL_VERSION: u32 = 5;

const _: () = assert!(MAX_FRAME_BYTES <= u32::MAX as usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    pub world_id: String,
    pub dimension_id: String,
    pub tick: u64,
    pub event_sequence: u64,
}

/// One snapshot cell. `id` is empty when the cell is not loaded; `data` is lowercase hex.
/// `states` are an owned block's state values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub pos: BlockPos,
    pub loaded: bool,
    pub id: String,
    pub owned: bool,
    pub data: Option<String>,
    pub states: Vec<BlockState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cause {
    Player,
    Guest,
    Environment,
}

/// A committed block change; `previous_data` is lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub pos: BlockPos,
    pub actor: Option<String>,
    pub cause: Cause,
    pub before_id: String,
    pub after_id: String,
    pub previous_data: Option<String>,
}

/// One value of a client-channel record, in the form the client part's wire protocol gives it: a
/// leaf, or a list or record of values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Scalar {
    Bool(bool),
    Integer(i64),
    Text(String),
    Choice(u16),
    /// The items of a list field, all of its one item type.
    List(Vec<Scalar>),
    /// One value per field of a record field, in order.
    Record(Vec<Scalar>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Call {
    Place {
        change: Change,
    },
    Break {
        change: Change,
    },
    Interact {
        player: String,
        pos: BlockPos,
        face: Face,
    },
    Neighbor {
        pos: BlockPos,
        neighbor: BlockPos,
    },
    /// `player`'s client part sent `payload` on `channel`. The callback's actor is `player`. With
    /// `focus`, the block of `player`'s focus, its snapshot is the one an interaction with that
    /// block would have; without, it is empty.
    ClientMessage {
        player: String,
        channel: String,
        schema: u16,
        payload: Vec<Scalar>,
        focus: Option<BlockPos>,
    },
    /// `player`'s client part moved to a new world epoch and kept running. The callback's actor
    /// is `player`, and its snapshot is that of `focus` like a client message's.
    Epoch {
        player: String,
        focus: Option<BlockPos>,
    },
}

/// Whether `id` is a canonical player id: a UUID in lowercase hyphenated form, hex digits in
/// groups of 8-4-4-4-12.
pub(crate) fn is_player_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
        })
}

/// Adapter → runtime. One request is decoded per frame and never stored in bulk, so the large
/// `Callback` variant stays inline.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// `items` are the server's items, which the guest may make stacks of besides its own.
    Load { dir: String, items: Vec<ServerItem> },
    Callback {
        seq: u64,
        info: Info,
        actor: Option<String>,
        world_min_y: i32,
        world_max_y: i32,
        data_budget: u64,
        snapshot: Vec<Cell>,
        /// The anchor's network, whose members `snapshot` holds, when the anchor is a member.
        network: Option<Network>,
        /// The actor's inventory, when the callback has an actor.
        inventory: Option<Inventory>,
        call: Call,
    },
    /// An empty struct variant, not a unit variant: serde ignores unknown fields on internally
    /// tagged unit variants even under `deny_unknown_fields`.
    Shutdown {},
}

/// A texture binding; `path` is absolute and validated by the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Texture {
    pub slot: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mining {
    /// Empty struct variant so unknown fields are rejected (see [`Request::Shutdown`]).
    Unbreakable {},
    Breakable {
        hardness: f32,
    },
}

/// A validated block: its 0.1 definition, then server WIT 0.5's states, placement traits, look
/// and network membership. A block with a `visual` binds no `textures`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDef {
    pub id: String,
    pub display_name: String,
    pub textures: Vec<Texture>,
    pub mining: Mining,
    pub states: Vec<StateDef>,
    pub placement: Vec<PlacementState>,
    pub visual: Option<Visual>,
    pub permutations: Vec<Permutation>,
    pub network: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailKind {
    Trap,
    Fuel,
    Deadline,
    Limit,
}

/// A staged operation; `data` is lowercase hex, `None` clears it. `SendClient` goes to the
/// player's client part after the rest commits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    SetBlock {
        pos: BlockPos,
        id: String,
    },
    SetBlockData {
        pos: BlockPos,
        data: Option<String>,
    },
    Tell {
        player: String,
        text: String,
    },
    SendClient {
        player: String,
        channel: String,
        schema: u16,
        payload: Vec<Scalar>,
    },
    /// New values for some states of an owned block, keeping its data and generation.
    SetBlockState {
        pos: BlockPos,
        states: Vec<BlockState>,
    },
    /// New content for one of the actor's inventory slots; `None` empties it.
    SetSlot {
        slot: u32,
        stack: Option<NewStack>,
    },
    /// An item entity holding `stack`, spawned at `pos`.
    DropItem {
        pos: BlockPos,
        stack: NewStack,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Committed {
        ops: Vec<Op>,
    },
    /// A guest error; not a strike.
    Rejected {
        reason: String,
    },
    /// A strike against the Experience.
    Failed {
        kind: FailKind,
        reason: String,
    },
}

/// Runtime → adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    /// `focus` is set when the Experience's world takes its player's focus in client messages
    /// and epochs; the adapter gives none to one that does not.
    Loaded {
        protocol: u32,
        id: String,
        version: String,
        blocks: Vec<BlockDef>,
        items: Vec<ItemDef>,
        focus: bool,
    },
    LoadFailed {
        reason: String,
    },
    Result {
        seq: u64,
        outcome: Outcome,
    },
}

/// `reason` cut to [`MAX_REASON_BYTES`] at a char boundary, so the answer that carries it fits in
/// a frame.
pub(crate) fn bounded_reason(mut reason: String) -> String {
    reason.truncate(reason.floor_char_boundary(MAX_REASON_BYTES));
    reason
}

/// Writes one frame and flushes. A message whose JSON exceeds [`MAX_FRAME_BYTES`] is rejected
/// with [`ErrorKind::InvalidInput`] before anything is written.
pub fn write_frame(w: &mut impl Write, msg: &impl Serialize) -> io::Result<()> {
    let mut frame = vec![0; 4];
    serde_json::to_writer(&mut frame, msg).map_err(io::Error::other)?;
    let length = frame.len() - 4;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("frame of {length} bytes exceeds {MAX_FRAME_BYTES}"),
        ));
    }
    frame[..4].copy_from_slice(&(length as u32).to_le_bytes());
    w.write_all(&frame)?;
    w.flush()
}

/// Reads one frame. Returns `Ok(None)` on a clean EOF before the length; a truncated frame is
/// [`ErrorKind::UnexpectedEof`], and an oversized or undecodable one is [`ErrorKind::InvalidData`].
pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<Option<T>> {
    let mut prefix = [0; 4];
    let mut filled = 0;
    while filled < prefix.len() {
        match r.read(&mut prefix[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "frame length is truncated",
                ));
            }
            Ok(read) => filled += read,
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    let length = u32::from_le_bytes(prefix) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("frame of {length} bytes exceeds {MAX_FRAME_BYTES}"),
        ));
    }
    let mut body = vec![0; length];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|err| io::Error::new(ErrorKind::InvalidData, err))
}

/// Constants the Go adapter must agree with: the frame limit, the protocol version, the limits
/// its commit check enforces again, and the client message limits that must fit the client wire
/// protocol's.
#[derive(Serialize)]
struct Limits {
    max_frame_bytes: usize,
    protocol: u32,
    max_block_data_bytes: usize,
    max_staged_ops: usize,
    max_tells: usize,
    max_tell_bytes: usize,
    max_client_sends: usize,
    max_client_send_bytes: usize,
    max_value_depth: usize,
    max_name_bytes: usize,
    max_state_values: usize,
    max_state_combinations: usize,
    max_bones: usize,
    max_permutations: usize,
    max_materials: usize,
    max_condition_tests: usize,
    max_flipbook_frames: usize,
    max_geometry_bytes: usize,
    max_network_blocks: usize,
    max_network_data_bytes: usize,
    inventory_slots: usize,
    hotbar_slots: u8,
    max_items: usize,
    max_stack_size: u8,
    max_item_data_bytes: usize,
    max_server_items: usize,
}

/// A placement trait's state and its values, in the client's order.
#[derive(Serialize)]
struct PlacementValues {
    placement: PlacementState,
    state: &'static str,
    values: &'static [&'static str],
}

/// Every protocol enum string, so the Go adapter can check its sets against Rust.
#[derive(Serialize)]
struct Enums {
    faces: [Face; 6],
    causes: [Cause; 3],
    fail_kinds: [FailKind; 4],
    render_methods: [RenderMethod; 4],
    placement_states: [PlacementState; 4],
    placement_values: Vec<PlacementValues>,
}

/// Lists every variant of a fieldless enum. The same list feeds an exhaustive `match`, so adding
/// a variant fails to compile until it is listed here.
macro_rules! all_variants {
    ($ty:ident: $($variant:ident),+ $(,)?) => {{
        let _exhaustive = |value: $ty| match value {
            $($ty::$variant)|+ => (),
        };
        [$($ty::$variant),+]
    }};
}

/// The golden fixtures, as `(file stem, pretty JSON with a trailing newline)`: one per
/// request, response, outcome, op and call variant, plus `limits` and `enums`.
pub fn fixtures() -> Vec<(&'static str, String)> {
    fn pretty(msg: &impl Serialize) -> String {
        serde_json::to_string_pretty(msg).expect("fixtures serialize") + "\n"
    }
    let player = "6f1c3c2e-5b7a-4d3e-9a51-0c8e2f4b7d10";
    let controller = BlockPos {
        x: 12,
        y: 64,
        z: -7,
    };
    let neighbor = BlockPos {
        x: 13,
        y: 64,
        z: -7,
    };
    let callback = |seq: u64, actor: Option<&str>, call: Call| Request::Callback {
        seq,
        info: Info {
            world_id: "world".to_owned(),
            dimension_id: "overworld".to_owned(),
            tick: 48_213,
            event_sequence: seq + 900,
        },
        actor: actor.map(str::to_owned),
        world_min_y: -64,
        world_max_y: 319,
        data_budget: 4096,
        snapshot: vec![
            Cell {
                pos: controller,
                loaded: true,
                id: "benergistics:controller".to_owned(),
                owned: true,
                data: Some(hex::encode(&[0x01, 0x00, 0xff])),
                states: vec![
                    state(
                        "benergistics:state",
                        StateValue::Choice("online".to_owned()),
                    ),
                    state("benergistics:powered", StateValue::Bool(true)),
                ],
            },
            Cell {
                pos: neighbor,
                loaded: true,
                id: "minecraft:stone".to_owned(),
                owned: false,
                data: None,
                states: Vec::new(),
            },
            Cell {
                pos: BlockPos {
                    x: 12,
                    y: 64,
                    z: 400,
                },
                loaded: false,
                id: String::new(),
                owned: false,
                data: None,
                states: Vec::new(),
            },
        ],
        network: None,
        inventory: None,
        call,
    };
    let result = |seq: u64, outcome: Outcome| Response::Result { seq, outcome };
    // A terminal right-click in a network: the anchor's network and the actor's inventory, a
    // stack of plain stone in hand and an Experience's own cell in the offhand.
    let mut slots = vec![None; 37];
    slots[0] = Some(ItemStack {
        id: "minecraft:stone".to_owned(),
        metadata: 0,
        count: 64,
        max_count: 64,
        data: None,
        plain: true,
    });
    slots[36] = Some(ItemStack {
        id: "benergistics:item_storage_cell_1k".to_owned(),
        metadata: 0,
        count: 1,
        max_count: 1,
        data: Some(hex::encode(&[0x01, 0x00, 0x00])),
        plain: false,
    });
    let mut networked = callback(
        7,
        Some(player),
        Call::Interact {
            player: player.to_owned(),
            pos: controller,
            face: Face::Up,
        },
    );
    if let Request::Callback {
        network, inventory, ..
    } = &mut networked
    {
        *network = Some(Network {
            blocks: vec![controller, neighbor],
            truncated: false,
        });
        *inventory = Some(Inventory { selected: 0, slots });
    }
    let new_stack = |id: &str, count: u8, data: Option<&[u8]>| NewStack {
        id: id.to_owned(),
        metadata: 0,
        count,
        data: data.map(hex::encode),
    };
    let pixel = |x: f32, y: f32, z: f32| Pixel { x, y, z };
    let north_on = vec![vec![StateTest {
        state: state("benergistics:north", StateValue::Bool(true)),
        equal: true,
    }]];
    // Every scalar type, the integer beyond what a JSON double holds exactly, and a list of
    // records, one of them empty, next to an empty list.
    let record = vec![
        Scalar::Bool(true),
        Scalar::Integer(-9_007_199_254_740_993),
        Scalar::Text("ME Controller \"linked\"".to_owned()),
        Scalar::Choice(2),
        Scalar::List(vec![
            Scalar::Record(vec![
                Scalar::Text("minecraft:iron_ingot".to_owned()),
                Scalar::Integer(64),
            ]),
            Scalar::Record(Vec::new()),
        ]),
        Scalar::List(Vec::new()),
    ];
    let without_snapshot = |mut request: Request| {
        if let Request::Callback { snapshot, .. } = &mut request {
            snapshot.clear();
        }
        request
    };
    let client_message = without_snapshot(callback(
        5,
        Some(player),
        Call::ClientMessage {
            player: player.to_owned(),
            channel: "benergistics.ack".to_owned(),
            schema: 1,
            payload: record.clone(),
            focus: None,
        },
    ));
    // An epoch with its player's focus has the snapshot of an interaction with that block.
    let epoch = callback(
        6,
        Some(player),
        Call::Epoch {
            player: player.to_owned(),
            focus: Some(controller),
        },
    );
    let asset = |file: &str| format!("/srv/experiences/benergistics/assets/{file}");
    let texture = |slot: &str, file: &str| Texture {
        slot: slot.to_owned(),
        path: format!("/srv/experiences/benergistics/assets/{file}"),
    };

    vec![
        (
            "request_load",
            pretty(&Request::Load {
                dir: "/srv/experiences/benergistics".to_owned(),
                items: vec![
                    ServerItem {
                        id: "minecraft:stone".to_owned(),
                        metadata: 0,
                        max_count: 64,
                        off_hand: false,
                        plain: true,
                    },
                    ServerItem {
                        id: "minecraft:ender_pearl".to_owned(),
                        metadata: 0,
                        max_count: 16,
                        off_hand: false,
                        plain: true,
                    },
                ],
            }),
        ),
        (
            "request_callback_place",
            pretty(&callback(
                1,
                Some(player),
                Call::Place {
                    change: Change {
                        pos: controller,
                        actor: Some(player.to_owned()),
                        cause: Cause::Player,
                        before_id: "minecraft:air".to_owned(),
                        after_id: "benergistics:controller".to_owned(),
                        previous_data: None,
                    },
                },
            )),
        ),
        (
            "request_callback_break",
            pretty(&callback(
                2,
                None,
                Call::Break {
                    change: Change {
                        pos: controller,
                        actor: None,
                        cause: Cause::Environment,
                        before_id: "benergistics:controller".to_owned(),
                        after_id: "minecraft:air".to_owned(),
                        previous_data: Some(hex::encode(&[0x01, 0x00, 0xff])),
                    },
                },
            )),
        ),
        (
            "request_callback_interact",
            pretty(&callback(
                3,
                Some(player),
                Call::Interact {
                    player: player.to_owned(),
                    pos: controller,
                    face: Face::North,
                },
            )),
        ),
        (
            "request_callback_neighbor",
            pretty(&callback(
                4,
                None,
                Call::Neighbor {
                    pos: controller,
                    neighbor,
                },
            )),
        ),
        ("request_callback_client_message", pretty(&client_message)),
        ("request_callback_epoch", pretty(&epoch)),
        ("request_callback_network", pretty(&networked)),
        ("request_shutdown", pretty(&Request::Shutdown {})),
        (
            "response_loaded",
            pretty(&Response::Loaded {
                protocol: PROTOCOL_VERSION,
                id: "benergistics".to_owned(),
                version: "0.1.0".to_owned(),
                blocks: vec![
                    BlockDef {
                        id: "benergistics:controller".to_owned(),
                        display_name: "ME Controller".to_owned(),
                        textures: vec![
                            texture("*", "controller.png"),
                            texture("up", "controller_powered.png"),
                        ],
                        mining: Mining::Breakable { hardness: 1.5 },
                        states: Vec::new(),
                        placement: Vec::new(),
                        visual: None,
                        permutations: Vec::new(),
                        network: true,
                    },
                    BlockDef {
                        id: "benergistics:creative_energy_cell".to_owned(),
                        display_name: "Creative Energy Cell".to_owned(),
                        textures: vec![texture("*", "creative_energy_cell.png")],
                        mining: Mining::Unbreakable {},
                        states: Vec::new(),
                        placement: Vec::new(),
                        visual: None,
                        permutations: Vec::new(),
                        network: true,
                    },
                    // A cable: six connection states shown by bone visibility, a flipbook, a
                    // placement trait and a permutation that turns it.
                    BlockDef {
                        id: "benergistics:glass_cable".to_owned(),
                        display_name: "ME Glass Cable".to_owned(),
                        textures: Vec::new(),
                        mining: Mining::Breakable { hardness: 0.5 },
                        states: vec![
                            StateDef {
                                name: "benergistics:north".to_owned(),
                                values: StateValues::Bool,
                            },
                            StateDef {
                                name: "benergistics:colour".to_owned(),
                                values: StateValues::Choices(vec![
                                    "fluix".to_owned(),
                                    "white".to_owned(),
                                ]),
                            },
                        ],
                        placement: vec![PlacementState::FacingDirection],
                        visual: Some(Visual {
                            geometry: Some(asset("glass_cable.geo.json")),
                            materials: vec![Material {
                                instance: "*".to_owned(),
                                path: asset("glass_cable.png"),
                                render_method: RenderMethod::AlphaTest,
                                flipbook: Some(Flipbook {
                                    ticks_per_frame: 25,
                                    frames: vec![0, 1, 2, 1],
                                    blend_frames: true,
                                }),
                            }],
                            bones: vec![BoneVisibility {
                                bone: "north".to_owned(),
                                visible: north_on.clone(),
                            }],
                            collision: Some(PixelBox {
                                min: pixel(6.5, 6.5, 6.5),
                                max: pixel(9.5, 9.5, 9.5),
                            }),
                            selection: None,
                            rotation: None,
                        }),
                        permutations: vec![Permutation {
                            when: vec![
                                north_on[0].clone(),
                                vec![StateTest {
                                    state: state(
                                        "benergistics:colour",
                                        StateValue::Choice("white".to_owned()),
                                    ),
                                    equal: false,
                                }],
                            ],
                            geometry: None,
                            materials: None,
                            bones: None,
                            collision: None,
                            selection: None,
                            rotation: Some(QuarterTurns { x: 0, y: 1, z: 0 }),
                        }],
                        network: true,
                    },
                ],
                items: vec![ItemDef {
                    id: "benergistics:item_storage_cell_1k".to_owned(),
                    display_name: "1k ME Item Storage Cell".to_owned(),
                    icon: asset("item_storage_cell_1k.png"),
                    max_stack: 1,
                }],
                focus: true,
            }),
        ),
        (
            "response_load_failed",
            pretty(&Response::LoadFailed {
                reason: "server.wasm: SHA-256 does not match experience.toml".to_owned(),
            }),
        ),
        (
            "response_result_committed",
            pretty(&result(
                1,
                Outcome::Committed {
                    ops: vec![
                        Op::SetBlock {
                            pos: neighbor,
                            id: "benergistics:creative_energy_cell".to_owned(),
                        },
                        Op::SetBlockData {
                            pos: controller,
                            data: Some(hex::encode(&[0x02, 0x10, 0xab])),
                        },
                        Op::SetBlockData {
                            pos: neighbor,
                            data: None,
                        },
                        Op::Tell {
                            player: player.to_owned(),
                            text: "Network online".to_owned(),
                        },
                        Op::SendClient {
                            player: player.to_owned(),
                            channel: "benergistics.controller".to_owned(),
                            schema: 1,
                            payload: record,
                        },
                        Op::SetBlockState {
                            pos: controller,
                            states: vec![state(
                                "benergistics:state",
                                StateValue::Choice("conflicted".to_owned()),
                            )],
                        },
                        Op::SetSlot {
                            slot: 0,
                            stack: Some(new_stack("minecraft:stone", 32, None)),
                        },
                        Op::SetSlot {
                            slot: 36,
                            stack: None,
                        },
                        Op::DropItem {
                            pos: neighbor,
                            stack: new_stack(
                                "benergistics:item_storage_cell_1k",
                                1,
                                Some(&[0x01, 0x00, 0x00]),
                            ),
                        },
                    ],
                },
            )),
        ),
        (
            "response_result_rejected",
            pretty(&result(
                2,
                Outcome::Rejected {
                    reason: "a controller already powers this network".to_owned(),
                },
            )),
        ),
        (
            "response_result_failed",
            pretty(&result(
                3,
                Outcome::Failed {
                    kind: FailKind::Fuel,
                    reason: "callback exhausted its fuel".to_owned(),
                },
            )),
        ),
        (
            "limits",
            pretty(&Limits {
                max_frame_bytes: MAX_FRAME_BYTES,
                protocol: PROTOCOL_VERSION,
                max_block_data_bytes: MAX_BLOCK_DATA_BYTES,
                max_staged_ops: MAX_STAGED_OPS,
                max_tells: MAX_TELLS,
                max_tell_bytes: MAX_TELL_BYTES,
                max_client_sends: MAX_CLIENT_SENDS,
                max_client_send_bytes: MAX_CLIENT_SEND_BYTES,
                max_value_depth: MAX_VALUE_DEPTH,
                max_name_bytes: MAX_NAME_BYTES,
                max_state_values: MAX_STATE_VALUES,
                max_state_combinations: MAX_STATE_COMBINATIONS,
                max_bones: MAX_BONES,
                max_permutations: MAX_PERMUTATIONS,
                max_materials: MAX_MATERIALS,
                max_condition_tests: MAX_CONDITION_TESTS,
                max_flipbook_frames: MAX_FLIPBOOK_FRAMES,
                max_geometry_bytes: MAX_GEOMETRY_BYTES,
                max_network_blocks: MAX_NETWORK_BLOCKS,
                max_network_data_bytes: MAX_NETWORK_DATA_BYTES,
                inventory_slots: INVENTORY_SLOTS,
                hotbar_slots: HOTBAR_SLOTS,
                max_items: MAX_ITEMS,
                max_stack_size: MAX_STACK_SIZE,
                max_item_data_bytes: MAX_ITEM_DATA_BYTES,
                max_server_items: MAX_SERVER_ITEMS,
            }),
        ),
        (
            "enums",
            pretty(&Enums {
                faces: all_variants!(Face: Down, Up, North, South, West, East),
                causes: all_variants!(Cause: Player, Guest, Environment),
                fail_kinds: all_variants!(FailKind: Trap, Fuel, Deadline, Limit),
                render_methods: all_variants!(
                    RenderMethod: Opaque,
                    AlphaTest,
                    Blend,
                    DoubleSided
                ),
                placement_states: all_variants!(
                    PlacementState: CardinalDirection,
                    FacingDirection,
                    BlockFace,
                    VerticalHalf
                ),
                placement_values: PlacementState::ALL
                    .into_iter()
                    .map(|placement| {
                        let (state, values) = placement.state();
                        PlacementValues {
                            placement,
                            state,
                            values,
                        }
                    })
                    .collect(),
            }),
        ),
    ]
}

/// A state named `name` holding `value`.
fn state(name: &str, value: StateValue) -> BlockState {
    BlockState {
        name: name.to_owned(),
        value,
    }
}

#[cfg(test)]
mod tests {
    use super::is_player_id;

    #[test]
    fn player_ids_are_canonical_uuids() {
        let canonical = [
            "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c",
            "00000000-0000-0000-0000-000000000000",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
        ];
        for id in canonical {
            assert!(is_player_id(id), "{id} is refused");
        }
        let other = [
            "",
            "3F2A7C1E-8B4D-4E6A-9C5F-1D2E3F4A5B6C",
            "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6C",
            "3f2a7c1e8b4d4e6a9c5f1d2e3f4a5b6c",
            "{3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c}",
            "urn:uuid:3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c",
            "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6",
            "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c0",
            "3f2a7c1e8-b4d-4e6a-9c5f-1d2e3f4a5b6c",
            "3f2a7c1g-8b4d-4e6a-9c5f-1d2e3f4a5b6c",
            // 36 bytes, with a two-byte character.
            "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5bé",
        ];
        for id in other {
            assert!(!is_player_id(id), "{id} is accepted");
        }
    }
}
