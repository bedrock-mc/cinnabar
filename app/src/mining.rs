//! Frozen interaction selection, targets and pick reach shared by mining and block use.

use std::num::NonZeroU64;

use protocol::{PlayerAuthInputInteractions, PlayerInputMode, VerifiedNetworkItemStack};
use semantic_input::InputMode;
use sim::WorldCollisionIdentity;

use crate::ui_runtime::{UiRuntime, inventory_ledger::PlayerInventorySlot};

/// Creative pick ranges observed for the three input modes the app exposes.
const CREATIVE_MOUSE_REACH_BLOCKS: f64 = 5.7;
const CREATIVE_GAMEPAD_REACH_BLOCKS: f64 = 5.6;
const CREATIVE_TOUCH_REACH_BLOCKS: f64 = 12.0;
/// Survival keeps the mouse and gamepad ranges; touch is shorter. Needs independent measurement.
const SURVIVAL_TOUCH_REACH_BLOCKS: f64 = 6.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrozenMiningFrame {
    pub(crate) session_generation: u64,
    pub(crate) position_authority_generation: u64,
    pub(crate) input_authority_generation: NonZeroU64,
    pub(crate) input_frame_sequence: u64,
    pub(crate) fifo_sequence: u64,
    pub(crate) physics_tick: u64,
    pub(crate) pose_generation: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenMiningRay {
    pub(crate) origin: [f32; 3],
    pub(crate) direction: [f32; 3],
    pub(crate) movement_world_identity: WorldCollisionIdentity,
    pub(crate) world_identity: WorldCollisionIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrozenMiningSelection {
    pub(crate) slot: u8,
    pub(crate) item: VerifiedNetworkItemStack,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenMiningTarget {
    pub(crate) position: [i32; 3],
    pub(crate) face: u8,
    pub(crate) relative_hit: [f32; 3],
    pub(crate) runtime_id: u32,
    pub(crate) identity: WorldCollisionIdentity,
}

/// One tick's committed block actions, item interaction and mining request.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QueuedMiningInteraction {
    pub(crate) interactions: PlayerAuthInputInteractions,
    pub(crate) mining_request: Option<protocol::MineBlockRequest>,
}

pub(crate) fn verified_selection(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    ui: &UiRuntime,
) -> Option<FrozenMiningSelection> {
    let selected = ui.selected_stack_snapshot(player_runtime)?;
    let stack = match selected.state {
        PlayerInventorySlot::Unknown => return None,
        PlayerInventorySlot::Empty => protocol::NetworkItemStack::empty(),
        PlayerInventorySlot::Present(stack) => stack.clone(),
    };
    let item = VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).ok()?;
    Some(FrozenMiningSelection {
        slot: selected.slot,
        item,
    })
}

/// Selection for bare-hand interactions (mining, melee): an unknown or empty
/// selected slot resolves to an empty hand, matching vanilla (assume empty
/// until restated), so hand actions work before the inventory arrives. Block
/// placement must not use this — it stays fail-closed via `verified_selection`.
pub(crate) fn hand_interaction_selection(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    ui: &UiRuntime,
) -> Option<FrozenMiningSelection> {
    let selected = ui.selected_stack_snapshot(player_runtime)?;
    let stack = match selected.state {
        PlayerInventorySlot::Unknown | PlayerInventorySlot::Empty => {
            protocol::NetworkItemStack::empty()
        }
        PlayerInventorySlot::Present(stack) => stack.clone(),
    };
    let item = VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).ok()?;
    Some(FrozenMiningSelection {
        slot: selected.slot,
        item,
    })
}

pub(crate) const fn protocol_input_mode(input_mode: InputMode) -> PlayerInputMode {
    match input_mode {
        InputMode::KeyboardMouse => PlayerInputMode::Mouse,
        InputMode::GamePad => PlayerInputMode::GamePad,
        InputMode::Touch => PlayerInputMode::Touch,
    }
}

pub(crate) const fn survival_reach(input_mode: PlayerInputMode) -> f64 {
    match input_mode {
        PlayerInputMode::Touch => SURVIVAL_TOUCH_REACH_BLOCKS,
        PlayerInputMode::Mouse | PlayerInputMode::GamePad => creative_reach(input_mode),
    }
}

pub(crate) const fn creative_reach(input_mode: PlayerInputMode) -> f64 {
    match input_mode {
        PlayerInputMode::Mouse => CREATIVE_MOUSE_REACH_BLOCKS,
        PlayerInputMode::GamePad => CREATIVE_GAMEPAD_REACH_BLOCKS,
        PlayerInputMode::Touch => CREATIVE_TOUCH_REACH_BLOCKS,
    }
}

#[cfg(test)]
mod tests;
