use std::ops::{BitOr, BitOrAssign};

use thiserror::Error;
use valentine::bedrock::version::v1_26_40::{
    EnumsClientPlayMode, EnumsInputMode, EnumsNewInteractionModel,
    EnumsPlayerAuthInputPacketPayloadInputData, PlayerAuthInputPacket, PlayerInputTick, Vec2, Vec3,
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
    #[error("PlayerAuthInput contains a non-finite position, rotation, delta, or input vector")]
    NonFiniteState,
}

/// Converts an app-owned movement snapshot to the pinned protocol-2168 packet.
pub fn player_auth_input(
    snapshot: PlayerAuthInputSnapshot,
) -> Result<Packet, PlayerAuthInputError> {
    let tick = snapshot.tick;
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

    Ok(PlayerAuthInputPacket {
        player_rotation: Vec2 {
            x: snapshot.pitch,
            y: snapshot.yaw,
        },
        position: vec3(snapshot.position),
        move_vector: vec2(snapshot.move_vector),
        player_head_rotation: snapshot.head_yaw,
        input_data: Some(input_data_items(snapshot.flags)),
        input_mode: match snapshot.input_mode {
            PlayerInputMode::Mouse => EnumsInputMode::Mouse,
            PlayerInputMode::Touch => EnumsInputMode::Touch,
            PlayerInputMode::GamePad => EnumsInputMode::GamePad,
        },
        play_mode: EnumsClientPlayMode::Normal,
        // 1.26.40 agrees with gophertunnel here, which writes the interaction
        // model with io.Varint32 (zigzag) in packet/player_auth_input.go. The
        // protocol-1001 code sent Unknown(-1) because the generated definition
        // was zigzag while the authority was an unsigned varint; that
        // workaround is obsolete and the named variant is now correct.
        new_interaction_model: EnumsNewInteractionModel::Crosshair,
        interact_rotation: Vec2 {
            x: snapshot.pitch,
            y: snapshot.yaw,
        },
        client_tick: PlayerInputTick { inputtick: tick },
        pos_delta: vec3(snapshot.delta),
        // These are the OUTER bool of each of gophertunnel's DoubleOptionalFunc
        // fields (minecraft/protocol/io.go): `outer := true; r.Bool(&outer);
        // if outer { OptionalFunc(...) }`. A Go writer can never emit false
        // here -- it is hardcoded true -- and the generated Option's own
        // presence byte is the inner flag that actually says "no payload".
        item_use_transaction: Some(None),
        item_stack_request: Some(None),
        player_block_actions: Some(None),
        vehicle_rotation: Some(None),
        client_predicted_vehicle: Some(None),
        analog_move_vector: vec2(snapshot.analogue_move_vector),
        camera_orientation: vec3(snapshot.camera_orientation),
        raw_move_vector: vec2(snapshot.raw_move_vector),
    }
    .into())
}

/// Expands the bitset the app owns into the flag list 1.26.40 puts on the wire.
///
/// The input flags stopped being a bitset and became a length-prefixed list of
/// the flag IDs that are set (gophertunnel's `protocol.InputFlagList`). Each
/// generated variant's ordinal is exactly the bit position the protocol-1001
/// bitset used, so bit `n` maps to the variant declared `n`th and the app-facing
/// `PlayerInputFlags` constants keep their meaning unchanged.
fn input_data_items(flags: PlayerInputFlags) -> Vec<EnumsPlayerAuthInputPacketPayloadInputData> {
    use EnumsPlayerAuthInputPacketPayloadInputData as Item;

    const ITEMS: [Item; 66] = [
        Item::Ascend,
        Item::Descend,
        Item::NorthJump,
        Item::JumpDown,
        Item::SprintDown,
        Item::ChangeHeight,
        Item::Jumping,
        Item::AutoJumpingInWater,
        Item::Sneaking,
        Item::SneakDown,
        Item::Up,
        Item::Down,
        Item::Left,
        Item::Right,
        Item::UpLeft,
        Item::UpRight,
        Item::WantUp,
        Item::WantDown,
        Item::WantDownSlow,
        Item::WantUpSlow,
        Item::Sprinting,
        Item::AscendBlock,
        Item::DescendBlock,
        Item::SneakToggleDown,
        Item::PersistSneak,
        Item::StartSprinting,
        Item::StopSprinting,
        Item::StartSneaking,
        Item::StopSneaking,
        Item::StartSwimming,
        Item::StopSwimming,
        Item::StartJumping,
        Item::StartGliding,
        Item::StopGliding,
        Item::PerformItemInteraction,
        Item::PerformBlockActions,
        Item::PerformItemStackRequest,
        Item::HandledTeleport,
        Item::Emoting,
        Item::MissedSwing,
        Item::StartCrawling,
        Item::StopCrawling,
        Item::StartFlying,
        Item::StopFlying,
        Item::ClientAckServerData,
        Item::IsInClientPredictedVehicle,
        Item::PaddlingLeft,
        Item::PaddlingRight,
        Item::BlockBreakingDelayEnabled,
        Item::HorizontalCollision,
        Item::VerticalCollision,
        Item::DownLeft,
        Item::DownRight,
        Item::StartUsingItem,
        Item::IsCameraRelativeMovementEnabled,
        Item::IsRotControlledByMoveDirection,
        Item::StartSpinAttack,
        Item::StopSpinAttack,
        Item::IsHotbarOnlyTouch,
        Item::JumpReleasedRaw,
        Item::JumpPressedRaw,
        Item::JumpCurrentRaw,
        Item::SneakReleasedRaw,
        Item::SneakPressedRaw,
        Item::SneakCurrentRaw,
        Item::InternalUpdate,
    ];

    let bits = flags.bits();
    (0..u64::BITS)
        .filter(|bit| bits & (1u64 << bit) != 0)
        .map(|bit| {
            ITEMS
                .get(bit as usize)
                .cloned()
                .unwrap_or(Item::Unknown(bit as i32))
        })
        .collect()
}

fn vec3(value: [f32; 3]) -> Vec3 {
    Vec3 {
        x: value[0],
        y: value[1],
        z: value[2],
    }
}

fn vec2(value: [f32; 2]) -> Vec2 {
    Vec2 {
        x: value[0],
        y: value[1],
    }
}
