//! Inventory adapter for frozen gameplay interaction selections.
use gameplay::mining::FrozenMiningSelection;
use inventory::inventory_ledger::PlayerInventorySlot;
use protocol::VerifiedNetworkItemStack;

/// Freezes the selected authoritative stack when its inventory slot is known.
pub fn verified_selection(
    player_runtime: &client_state::PlayerRuntime,
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
pub fn hand_interaction_selection(
    player_runtime: &client_state::PlayerRuntime,
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

/// Eligibility used by a retained interaction selection.
#[derive(Clone, Copy)]
enum SelectionKind {
    Hand,
    Use,
}

/// Canonical selected descriptor and authoritative cancellation evidence for one attempt.
#[derive(Clone)]
pub struct SelectionObservation {
    pub selection: FrozenMiningSelection,
    stamp: inventory::SelectionStamp,
    kind: SelectionKind,
}

impl SelectionObservation {
    /// Captures bare-hand eligibility without treating an unknown cell as a usable item.
    pub fn hand(player: &client_state::PlayerRuntime) -> Option<Self> {
        Some(Self::capture(
            player,
            hand_interaction_selection(player)?,
            SelectionKind::Hand,
        ))
    }
    /// Captures verified use eligibility, including gesture and pending-selection fences.
    pub fn for_use(
        player: &client_state::PlayerRuntime,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) -> Option<Self> {
        Some(Self::capture(
            player,
            crate::block_use::verified_use_selection(player, ui)?,
            SelectionKind::Use,
        ))
    }
    /// Associates one resolved descriptor with the owning inventory cell's lifetime.
    fn capture(
        player: &client_state::PlayerRuntime,
        selection: FrozenMiningSelection,
        kind: SelectionKind,
    ) -> Self {
        let stamp = player.inventory.selection_stamp(selection.slot);
        Self {
            selection,
            stamp,
            kind,
        }
    }
    /// Checks current authority, cell contents and the original interaction eligibility in one place.
    pub fn current(
        &self,
        player: &client_state::PlayerRuntime,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) -> bool {
        let selection = match self.kind {
            SelectionKind::Hand => hand_interaction_selection(player),
            SelectionKind::Use => crate::block_use::verified_use_selection(player, ui),
        };
        player.inventory.selection_stamp_current(self.stamp)
            && selection.as_ref() == Some(&self.selection)
    }
}

#[cfg(test)]
mod tests;
