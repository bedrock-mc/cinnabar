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
    MAX_REASON_BYTES, MAX_RECIPE_INPUT_ITEMS, MAX_RECIPES, MAX_SCHEDULE_TICKS,
    MAX_SCHEDULED_UPDATES, MAX_SERVER_ITEMS, MAX_STACK_SIZE, MAX_STAGED_OPS,
    MAX_STATE_COMBINATIONS, MAX_STATE_VALUES, MAX_TELL_BYTES, MAX_TELLS, MAX_VALUE_DEPTH,
};

mod player;
mod recipes;
mod visuals;

pub use recipes::{Crafted, ItemMatch, Recipe};

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
/// and visuals, items, the actor's inventory, the network scope and their ops; 6 server WIT 0.6's
/// recipes in `load`, the `scheduled_update` call and the `schedule_update` op.
pub const PROTOCOL_VERSION: u32 = 6;

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
    /// An update the Experience scheduled for its block at `pos` is due. The snapshot is that of
    /// an interaction with the block.
    ScheduledUpdate {
        pos: BlockPos,
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
    /// `recipes` are the server's crafting-table recipes, in the order the server tries them.
    Load {
        dir: String,
        items: Vec<ServerItem>,
        recipes: Vec<Recipe>,
    },
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
    /// An update of the owned block at `pos`, `ticks` game ticks after the result commits.
    ScheduleUpdate {
        pos: BlockPos,
        ticks: u32,
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

mod fixtures;
pub use fixtures::fixtures;

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
