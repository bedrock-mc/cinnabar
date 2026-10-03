//! The adapter protocol: message types, framing and the golden fixtures the Go adapter checks.
//!
//! A frame is a 4-byte little-endian length followed by that many bytes of JSON. Bytes inside
//! messages are lowercase hex (see [`crate::hex`]).

use std::io::{self, ErrorKind, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::hex;
use crate::limits::{
    MAX_BLOCK_DATA_BYTES, MAX_CLIENT_SENDS, MAX_FRAME_BYTES, MAX_REASON_BYTES, MAX_STAGED_OPS,
    MAX_TELL_BYTES, MAX_TELLS,
};

/// 2 added the `client_message` call and the `send_client` op.
pub const PROTOCOL_VERSION: u32 = 2;

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub pos: BlockPos,
    pub loaded: bool,
    pub id: String,
    pub owned: bool,
    pub data: Option<String>,
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

/// One field of a client-channel record, in the form the client part's wire protocol gives it.
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
    /// `player`'s client part sent `payload` on `channel`. The callback's actor is `player`, and
    /// its snapshot is empty.
    ClientMessage {
        player: String,
        channel: String,
        schema: u16,
        payload: Vec<Scalar>,
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
    Load {
        dir: String,
    },
    Callback {
        seq: u64,
        info: Info,
        actor: Option<String>,
        world_min_y: i32,
        world_max_y: i32,
        data_budget: u64,
        snapshot: Vec<Cell>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDef {
    pub id: String,
    pub display_name: String,
    pub textures: Vec<Texture>,
    pub mining: Mining,
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
    Loaded {
        protocol: u32,
        id: String,
        version: String,
        blocks: Vec<BlockDef>,
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

/// Constants the Go adapter must agree with: the frame limit, the protocol version, and the
/// limits its commit check enforces again.
#[derive(Serialize)]
struct Limits {
    max_frame_bytes: usize,
    protocol: u32,
    max_block_data_bytes: usize,
    max_staged_ops: usize,
    max_tells: usize,
    max_tell_bytes: usize,
    max_client_sends: usize,
}

/// Every protocol enum string, so the Go adapter can check its sets against Rust.
#[derive(Serialize)]
struct Enums {
    faces: [Face; 6],
    causes: [Cause; 3],
    fail_kinds: [FailKind; 4],
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
            },
            Cell {
                pos: neighbor,
                loaded: true,
                id: "minecraft:stone".to_owned(),
                owned: false,
                data: None,
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
            },
        ],
        call,
    };
    let result = |seq: u64, outcome: Outcome| Response::Result { seq, outcome };
    // Every scalar type; the integer is beyond what a JSON double holds exactly.
    let record = vec![
        Scalar::Bool(true),
        Scalar::Integer(-9_007_199_254_740_993),
        Scalar::Text("ME Controller \"linked\"".to_owned()),
        Scalar::Choice(2),
    ];
    let mut client_message = callback(
        5,
        Some(player),
        Call::ClientMessage {
            player: player.to_owned(),
            channel: "benergistics.ack".to_owned(),
            schema: 1,
            payload: record.clone(),
        },
    );
    if let Request::Callback { snapshot, .. } = &mut client_message {
        snapshot.clear();
    }
    let texture = |slot: &str, file: &str| Texture {
        slot: slot.to_owned(),
        path: format!("/srv/experiences/benergistics/assets/{file}"),
    };

    vec![
        (
            "request_load",
            pretty(&Request::Load {
                dir: "/srv/experiences/benergistics".to_owned(),
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
                    },
                    BlockDef {
                        id: "benergistics:creative_energy_cell".to_owned(),
                        display_name: "Creative Energy Cell".to_owned(),
                        textures: vec![texture("*", "creative_energy_cell.png")],
                        mining: Mining::Unbreakable {},
                    },
                ],
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
            }),
        ),
        (
            "enums",
            pretty(&Enums {
                faces: all_variants!(Face: Down, Up, North, South, West, East),
                causes: all_variants!(Cause: Player, Guest, Environment),
                fail_kinds: all_variants!(FailKind: Trap, Fuel, Deadline, Limit),
            }),
        ),
    ]
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
