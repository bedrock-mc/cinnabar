//! App inventory adapter for frozen gameplay interaction selections.
use client_ui::ui_runtime::inventory_ledger::PlayerInventorySlot;
pub(crate) use gameplay::mining::{
    FrozenMiningFrame, FrozenMiningRay, FrozenMiningSelection, FrozenMiningTarget, creative_reach,
    protocol_input_mode, survival_reach,
};
use protocol::VerifiedNetworkItemStack;

pub(crate) fn verified_selection(
    player_runtime: &crate::player_runtime::PlayerRuntime,
) -> Option<FrozenMiningSelection> {
    let selected = player_runtime.selected_stack_snapshot()?;
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
) -> Option<FrozenMiningSelection> {
    let selected = player_runtime.selected_stack_snapshot()?;
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

#[cfg(test)]
mod tests;
