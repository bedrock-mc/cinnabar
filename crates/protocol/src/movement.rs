use std::ops::{BitOr, BitOrAssign};

use thiserror::Error;
use valentine::bedrock::version::v1_26_44::{
    EnumsClientPlayMode, EnumsInputMode, EnumsNewInteractionModel,
    EnumsPlayerAuthInputPacketPayloadInputData, PlayerAuthInputPacket, PlayerInputTick, Vec2, Vec3,
};

mod interactions;
mod trace;

pub use interactions::{
    BlockAction, BlockActionKind, BlockActions, BlockActionsFull, InteractionEncodeError,
    MAX_BLOCK_ACTIONS_PER_INPUT, PlayerAuthInputInteractions,
};
pub use trace::{PlayerAuthInputTraceSample, player_auth_input_trace_sample};

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
    pub const UP_LEFT: Self = Self(1 << 14);
    pub const UP_RIGHT: Self = Self(1 << 15);
    pub const SPRINTING: Self = Self(1 << 20);
    pub const START_SPRINTING: Self = Self(1 << 25);
    pub const STOP_SPRINTING: Self = Self(1 << 26);
    pub const START_SNEAKING: Self = Self(1 << 27);
    pub const STOP_SNEAKING: Self = Self(1 << 28);
    pub const START_JUMPING: Self = Self(1 << 31);
    /// Wire ordinal 34 (`PerformItemInteraction`): the packet carries an
    /// embedded item-use transaction. Derived from payload presence by the
    /// encoder; callers never assert it directly.
    pub const PERFORM_ITEM_INTERACTION: Self = Self(1 << 34);
    /// Wire ordinal 35 (`PerformBlockActions`): the packet carries a
    /// block-action list. Derived from payload presence by the encoder;
    /// callers never assert it directly.
    pub const PERFORM_BLOCK_ACTIONS: Self = Self(1 << 35);
    /// Wire ordinal 37 of the input-data list (`HandledTeleport`). The app
    /// asserts this flag on the first transmitted sample after a qualifying
    /// server teleport; see the movement `teleport_ack` module.
    pub const HANDLED_TELEPORT: Self = Self(1 << 37);
    pub const HORIZONTAL_COLLISION: Self = Self(1 << 49);
    pub const VERTICAL_COLLISION: Self = Self(1 << 50);
    pub const DOWN_LEFT: Self = Self(1 << 51);
    pub const DOWN_RIGHT: Self = Self(1 << 52);
    pub const JUMP_RELEASED_RAW: Self = Self(1 << 59);
    pub const JUMP_PRESSED_RAW: Self = Self(1 << 60);
    pub const JUMP_CURRENT_RAW: Self = Self(1 << 61);
    pub const SNEAK_RELEASED_RAW: Self = Self(1 << 62);
    pub const SNEAK_PRESSED_RAW: Self = Self(1 << 63);
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn with_mask(self, mask: Self, enabled: bool) -> Self {
        if enabled {
            Self(self.0 | mask.0)
        } else {
            Self(self.0 & !mask.0)
        }
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
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PlayerAuthInputError {
    #[error("PlayerAuthInput contains a non-finite position, rotation, delta, or input vector")]
    NonFiniteState,
    #[error("PlayerAuthInput interactions are invalid: {0}")]
    Interaction(#[from] InteractionEncodeError),
}

/// Converts an app-owned movement snapshot to the pinned protocol-2168 packet
/// without any interaction payload.
pub fn player_auth_input(
    snapshot: PlayerAuthInputSnapshot,
) -> Result<Packet, PlayerAuthInputError> {
    player_auth_input_with_interactions(snapshot, &PlayerAuthInputInteractions::default())
}

/// Converts an app-owned movement snapshot plus the interactions of the same
/// tick to the pinned protocol-2168 packet.
///
/// The `PerformBlockActions` and `PerformItemInteraction` flags are derived
/// exclusively from payload presence so the flag list and the optional
/// payloads can never disagree on the wire; a snapshot that asserts either
/// flag itself is rejected.
pub fn player_auth_input_with_interactions(
    snapshot: PlayerAuthInputSnapshot,
    interactions: &PlayerAuthInputInteractions,
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
    let derived_flag_bits = PlayerInputFlags::PERFORM_ITEM_INTERACTION.bits()
        | PlayerInputFlags::PERFORM_BLOCK_ACTIONS.bits();
    if snapshot.flags.bits() & derived_flag_bits != 0 {
        return Err(InteractionEncodeError::InconsistentInteractionFlags.into());
    }
    let mut flags = snapshot.flags;
    let player_block_actions = if interactions.block_actions.is_empty() {
        None
    } else {
        flags |= PlayerInputFlags::PERFORM_BLOCK_ACTIONS;
        Some(interactions.block_actions.vendor()?)
    };
    let item_use_transaction = match &interactions.block_destroy {
        None => None,
        Some(request) => {
            flags |= PlayerInputFlags::PERFORM_ITEM_INTERACTION;
            Some(interactions::packed_block_destroy(request.clone())?)
        }
    };

    Ok(PlayerAuthInputPacket {
        player_rotation: Vec2 {
            x: snapshot.pitch,
            y: snapshot.yaw,
        },
        position: vec3(snapshot.position),
        move_vector: vec2(snapshot.move_vector),
        player_head_rotation: snapshot.head_yaw,
        input_data: Some(input_data_items(flags)),
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
        item_use_transaction: Some(item_use_transaction),
        item_stack_request: Some(None),
        player_block_actions: Some(player_block_actions),
        vehicle_rotation: Some(None),
        client_predicted_vehicle: Some(None),
        analog_move_vector: vec2(snapshot.analogue_move_vector),
        camera_orientation: vec3(snapshot.camera_orientation),
        raw_move_vector: vec2(snapshot.raw_move_vector),
    }
    .into())
}

use EnumsPlayerAuthInputPacketPayloadInputData as Item;

/// One pinned input flag: its generated wire variant paired with the exact
/// diagnostic name used by [`player_auth_input_trace_sample`]. Row `n` is bit
/// `n`; each generated variant's ordinal equals its row, so keeping the name
/// beside the variant in one table is what keeps trace names from drifting
/// away from the encoder's spelling.
type InputFlagItem = EnumsPlayerAuthInputPacketPayloadInputData;

const INPUT_FLAG_ITEMS: [(InputFlagItem, &str); 66] = [
    (Item::Ascend, "Ascend"),
    (Item::Descend, "Descend"),
    (Item::NorthJump, "NorthJump"),
    (Item::JumpDown, "JumpDown"),
    (Item::SprintDown, "SprintDown"),
    (Item::ChangeHeight, "ChangeHeight"),
    (Item::Jumping, "Jumping"),
    (Item::AutoJumpingInWater, "AutoJumpingInWater"),
    (Item::Sneaking, "Sneaking"),
    (Item::SneakDown, "SneakDown"),
    (Item::Up, "Up"),
    (Item::Down, "Down"),
    (Item::Left, "Left"),
    (Item::Right, "Right"),
    (Item::UpLeft, "UpLeft"),
    (Item::UpRight, "UpRight"),
    (Item::WantUp, "WantUp"),
    (Item::WantDown, "WantDown"),
    (Item::WantDownSlow, "WantDownSlow"),
    (Item::WantUpSlow, "WantUpSlow"),
    (Item::Sprinting, "Sprinting"),
    (Item::AscendBlock, "AscendBlock"),
    (Item::DescendBlock, "DescendBlock"),
    (Item::SneakToggleDown, "SneakToggleDown"),
    (Item::PersistSneak, "PersistSneak"),
    (Item::StartSprinting, "StartSprinting"),
    (Item::StopSprinting, "StopSprinting"),
    (Item::StartSneaking, "StartSneaking"),
    (Item::StopSneaking, "StopSneaking"),
    (Item::StartSwimming, "StartSwimming"),
    (Item::StopSwimming, "StopSwimming"),
    (Item::StartJumping, "StartJumping"),
    (Item::StartGliding, "StartGliding"),
    (Item::StopGliding, "StopGliding"),
    (Item::PerformItemInteraction, "PerformItemInteraction"),
    (Item::PerformBlockActions, "PerformBlockActions"),
    (Item::PerformItemStackRequest, "PerformItemStackRequest"),
    (Item::HandledTeleport, "HandledTeleport"),
    (Item::Emoting, "Emoting"),
    (Item::MissedSwing, "MissedSwing"),
    (Item::StartCrawling, "StartCrawling"),
    (Item::StopCrawling, "StopCrawling"),
    (Item::StartFlying, "StartFlying"),
    (Item::StopFlying, "StopFlying"),
    (Item::ClientAckServerData, "ClientAckServerData"),
    (
        Item::IsInClientPredictedVehicle,
        "IsInClientPredictedVehicle",
    ),
    (Item::PaddlingLeft, "PaddlingLeft"),
    (Item::PaddlingRight, "PaddlingRight"),
    (Item::BlockBreakingDelayEnabled, "BlockBreakingDelayEnabled"),
    (Item::HorizontalCollision, "HorizontalCollision"),
    (Item::VerticalCollision, "VerticalCollision"),
    (Item::DownLeft, "DownLeft"),
    (Item::DownRight, "DownRight"),
    (Item::StartUsingItem, "StartUsingItem"),
    (
        Item::IsCameraRelativeMovementEnabled,
        "IsCameraRelativeMovementEnabled",
    ),
    (
        Item::IsRotControlledByMoveDirection,
        "IsRotControlledByMoveDirection",
    ),
    (Item::StartSpinAttack, "StartSpinAttack"),
    (Item::StopSpinAttack, "StopSpinAttack"),
    (Item::IsHotbarOnlyTouch, "IsHotbarOnlyTouch"),
    (Item::JumpReleasedRaw, "JumpReleasedRaw"),
    (Item::JumpPressedRaw, "JumpPressedRaw"),
    (Item::JumpCurrentRaw, "JumpCurrentRaw"),
    (Item::SneakReleasedRaw, "SneakReleasedRaw"),
    (Item::SneakPressedRaw, "SneakPressedRaw"),
    (Item::SneakCurrentRaw, "SneakCurrentRaw"),
    (Item::InternalUpdate, "InternalUpdate"),
];

/// Expands the bitset the app owns into the flag list 1.26.40 puts on the wire.
///
/// The input flags stopped being a bitset and became a length-prefixed list of
/// the flag IDs that are set (gophertunnel's `protocol.InputFlagList`). Each
/// generated variant's ordinal is exactly the bit position the protocol-1001
/// bitset used, so bit `n` maps to the table row declared `n`th and the
/// app-facing [`PlayerInputFlags`] constants keep their meaning unchanged.
fn input_data_items(flags: PlayerInputFlags) -> Vec<EnumsPlayerAuthInputPacketPayloadInputData> {
    let bits = flags.bits();
    (0..u64::BITS)
        .filter(|bit| bits & (1u64 << bit) != 0)
        .map(|bit| {
            INPUT_FLAG_ITEMS
                .get(bit as usize)
                .map(|(item, _name)| *item)
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
