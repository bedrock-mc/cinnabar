use std::ops::{BitOr, BitOrAssign};

use thiserror::Error;
use valentine::bedrock::version::v1_26_30::{
    InputFlag, PlayerAuthInputPacket, PlayerAuthInputPacketInputMode,
    PlayerAuthInputPacketInteractionModel, PlayerAuthInputPacketPlayMode, Vec2F, Vec3F,
};

use crate::Packet;

/// Input flags exposed to the app without leaking the generated Valentine packet API.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerInputFlags(u64);

impl PlayerInputFlags {
    pub const NONE: Self = Self(0);
    pub const JUMP_DOWN: Self = Self(1 << 3);
    pub const SPRINT_DOWN: Self = Self(1 << 4);
    pub const JUMPING: Self = Self(1 << 6);
    pub const SNEAKING: Self = Self(1 << 8);
    pub const SNEAK_DOWN: Self = Self(1 << 9);
    pub const UP: Self = Self(1 << 10);
    pub const DOWN: Self = Self(1 << 11);
    pub const LEFT: Self = Self(1 << 12);
    pub const RIGHT: Self = Self(1 << 13);
    pub const SPRINTING: Self = Self(1 << 20);
    pub const START_SPRINTING: Self = Self(1 << 25);
    pub const STOP_SPRINTING: Self = Self(1 << 26);
    pub const START_SNEAKING: Self = Self(1 << 27);
    pub const STOP_SNEAKING: Self = Self(1 << 28);
    pub const START_JUMPING: Self = Self(1 << 31);
    pub const JUMP_RELEASED_RAW: Self = Self(1 << 59);
    pub const JUMP_PRESSED_RAW: Self = Self(1 << 60);
    pub const JUMP_CURRENT_RAW: Self = Self(1 << 61);
    pub const SNEAK_RELEASED_RAW: Self = Self(1 << 62);
    pub const SNEAK_PRESSED_RAW: Self = Self(1 << 63);

    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }
}

impl BitOr for PlayerInputFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for PlayerInputFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Physical input source reported to Bedrock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlayerInputMode {
    #[default]
    Mouse,
    Touch,
    GamePad,
}

/// One deterministic movement-tick snapshot sent to a server-authoritative server.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerAuthInputSnapshot {
    pub tick: u64,
    pub position: [f32; 3],
    pub delta: [f32; 3],
    pub move_vector: [f32; 2],
    pub analogue_move_vector: [f32; 2],
    pub raw_move_vector: [f32; 2],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub camera_orientation: [f32; 3],
    pub flags: PlayerInputFlags,
    pub input_mode: PlayerInputMode,
}

/// Invalid app-owned state that cannot be represented safely on the wire.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum PlayerAuthInputError {
    #[error("PlayerAuthInput tick {0} exceeds the protocol-1001 signed wire range")]
    TickOutOfRange(u64),
    #[error("PlayerAuthInput contains a non-finite position, rotation, delta, or input vector")]
    NonFiniteState,
}

/// Converts an app-owned movement snapshot to the pinned protocol-1001 packet.
pub fn player_auth_input(
    snapshot: PlayerAuthInputSnapshot,
) -> Result<Packet, PlayerAuthInputError> {
    let tick = i64::try_from(snapshot.tick)
        .map_err(|_| PlayerAuthInputError::TickOutOfRange(snapshot.tick))?;
    let finite = snapshot
        .position
        .into_iter()
        .chain(snapshot.delta)
        .chain(snapshot.move_vector)
        .chain(snapshot.analogue_move_vector)
        .chain(snapshot.raw_move_vector)
        .chain([snapshot.pitch, snapshot.yaw, snapshot.head_yaw])
        .chain(snapshot.camera_orientation)
        .all(f32::is_finite);
    if !finite {
        return Err(PlayerAuthInputError::NonFiniteState);
    }

    let move_vector = vec2(snapshot.move_vector);
    Ok(PlayerAuthInputPacket {
        pitch: snapshot.pitch,
        yaw: snapshot.yaw,
        position: vec3(snapshot.position),
        move_vector: move_vector.clone(),
        head_yaw: snapshot.head_yaw,
        input_data: InputFlag::from_bits_retain(snapshot.flags.bits()),
        input_mode: match snapshot.input_mode {
            PlayerInputMode::Mouse => PlayerAuthInputPacketInputMode::Mouse,
            PlayerInputMode::Touch => PlayerAuthInputPacketInputMode::Touch,
            PlayerInputMode::GamePad => PlayerAuthInputPacketInputMode::GamePad,
        },
        play_mode: PlayerAuthInputPacketPlayMode::Normal,
        // Gophertunnel's protocol-1001 authority writes this field as unsigned
        // varint. The pinned generated Valentine definition currently labels it
        // ZigZag32, so -1 is the generated representation whose wire bytes are
        // the authoritative unsigned value 1 (crosshair). Keep this workaround
        // contained behind the vendor-neutral snapshot API.
        interaction_model: PlayerAuthInputPacketInteractionModel::Unknown(-1),
        interact_rotation: Vec2F {
            x: snapshot.pitch,
            z: snapshot.yaw,
        },
        tick,
        delta: vec3(snapshot.delta),
        transaction: None,
        item_stack_request: None,
        content: None,
        block_action: None,
        analogue_move_vector: vec2(snapshot.analogue_move_vector),
        camera_orientation: vec3(snapshot.camera_orientation),
        raw_move_vector: vec2(snapshot.raw_move_vector),
    }
    .into())
}

fn vec3(value: [f32; 3]) -> Vec3F {
    Vec3F {
        x: value[0],
        y: value[1],
        z: value[2],
    }
}

fn vec2(value: [f32; 2]) -> Vec2F {
    Vec2F {
        x: value[0],
        z: value[1],
    }
}
