//! Inventory adapter for frozen gameplay interaction selections.
use gameplay::mining::FrozenMiningSelection;
use inventory::inventory_ledger::PlayerInventorySlot;
use protocol::VerifiedNetworkItemStack;

/// Freezes selected player backing when its inventory slot is known.
pub fn verified_selection(
    player_runtime: &client_state::PlayerRuntime,
) -> Option<FrozenMiningSelection> {
    let selected = player_runtime
        .inventory
        .selected_backing_stack_snapshot(player_runtime.facts.player_game_mode())?;
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
/// Unsent backing mutations and hotbar announcements defer hand actions.
pub fn hand_interaction_selection(
    player_runtime: &client_state::PlayerRuntime,
) -> Option<FrozenMiningSelection> {
    let selected = player_runtime
        .inventory
        .selected_backing_stack_snapshot(player_runtime.facts.player_game_mode())?;
    if player_runtime
        .inventory
        .pending_hotbar_selection()
        .is_some()
        || player_runtime
            .inventory
            .ledger()
            .slot_backing_transport_pending(selected.slot)
    {
        return None;
    }
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
