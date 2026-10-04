//! Frozen interaction selection, targets and pick reach shared by mining and block use.

use std::num::NonZeroU64;

use protocol::{PlayerAuthInputInteractions, PlayerInputMode, VerifiedNetworkItemStack};
use semantic_input::InputMode;
use sim::WorldCollisionIdentity;

/// Creative pick ranges observed for the three input modes the app exposes.
const CREATIVE_MOUSE_REACH_BLOCKS: f64 = 5.7;
const CREATIVE_GAMEPAD_REACH_BLOCKS: f64 = 5.6;
const CREATIVE_TOUCH_REACH_BLOCKS: f64 = 12.0;
/// Survival keeps the mouse and gamepad ranges; touch is shorter. Needs independent measurement.
const SURVIVAL_TOUCH_REACH_BLOCKS: f64 = 6.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrozenMiningFrame {
    pub session_generation: u64,
    pub position_authority_generation: u64,
    pub input_authority_generation: NonZeroU64,
    pub input_frame_sequence: u64,
    pub fifo_sequence: u64,
    pub physics_tick: u64,
    pub pose_generation: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenMiningRay {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub movement_world_identity: WorldCollisionIdentity,
    pub world_identity: WorldCollisionIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenMiningSelection {
    pub slot: u8,
    pub item: VerifiedNetworkItemStack,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenMiningTarget {
    pub position: [i32; 3],
    pub face: u8,
    pub relative_hit: [f32; 3],
    pub runtime_id: u32,
    pub identity: WorldCollisionIdentity,
}

/// One tick's committed block actions, item interaction and mining request.
#[derive(Debug, Clone, PartialEq)]
pub struct QueuedMiningInteraction {
    pub interactions: PlayerAuthInputInteractions,
    pub mining_request: Option<protocol::MineBlockRequest>,
}

pub const fn protocol_input_mode(input_mode: InputMode) -> PlayerInputMode {
    match input_mode {
        InputMode::KeyboardMouse => PlayerInputMode::Mouse,
        InputMode::GamePad => PlayerInputMode::GamePad,
        InputMode::Touch => PlayerInputMode::Touch,
    }
}

pub const fn survival_reach(input_mode: PlayerInputMode) -> f64 {
    match input_mode {
        PlayerInputMode::Touch => SURVIVAL_TOUCH_REACH_BLOCKS,
        PlayerInputMode::Mouse | PlayerInputMode::GamePad => creative_reach(input_mode),
    }
}

pub const fn creative_reach(input_mode: PlayerInputMode) -> f64 {
    match input_mode {
        PlayerInputMode::Mouse => CREATIVE_MOUSE_REACH_BLOCKS,
        PlayerInputMode::GamePad => CREATIVE_GAMEPAD_REACH_BLOCKS,
        PlayerInputMode::Touch => CREATIVE_TOUCH_REACH_BLOCKS,
    }
}
