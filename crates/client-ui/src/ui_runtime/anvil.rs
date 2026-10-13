//! The open anvil's predicted result and cost, and the name box it prefills.

use inventory::inventory_ledger::{ANVIL_MATERIAL_SLOT, ANVIL_TARGET_SLOT, InventoryTarget};
use protocol::{ItemRegistryEntry, NetworkItemStack, WindowKind};
use sha2::{Digest, Sha256};

use super::UiRuntime;

/// The anvil's predicted result stack and what taking it costs.
#[derive(Clone, Debug, PartialEq)]
pub struct AnvilPreview {
    pub stack: NetworkItemStack,
    pub outcome: inventory::AnvilOutcome,
}

/// One occupied anvil input: its stack, its registry entry and its decoded display data.
struct AnvilInput {
    stack: NetworkItemStack,
    entry: ItemRegistryEntry,
    display: protocol::ItemDisplay,
}

impl AnvilInput {
    /// The durability the registry gives this item: its components' maximum, else the
    /// vanilla maximum for an item the server does not define by components.
    fn max_damage(&self) -> Option<u32> {
        match &self.entry.components {
            Some(components) => components.max_durability.filter(|maximum| *maximum > 0),
            None if self.entry.component_based => None,
            None => {
                assets::item::durability::builtin_item_durability(&self.entry.identifier).maximum()
            }
        }
    }

    /// The calculator's view of this input.
    fn item(&self) -> inventory::AnvilItem<'_> {
        inventory::AnvilItem {
            identifier: &self.entry.identifier,
            count: self.stack.count,
            damage: protocol::item_stack_damage(&self.stack).unwrap_or(0),
            max_damage: self.max_damage(),
            enchantments: &self.display.enchantments,
            repair_cost: protocol::item_repair_cost(&self.stack.extra_data),
            custom_name: self.display.name.as_deref(),
        }
    }
}

impl UiRuntime {
    /// The occupied anvil input in UI `slot`, when the item is in the session registry.
    fn anvil_input(
        &self,
        player_runtime: &player_state::PlayerState,
        slot: u8,
    ) -> Option<AnvilInput> {
        let ledger = self.inventory_ledger(player_runtime);
        let stack = ledger
            .target_stack(InventoryTarget::Craft(slot))
            .filter(|stack| stack.count > 0)?
            .clone();
        let entry = ledger.negotiated_item_entry(stack.network_id)?.clone();
        let display = protocol::item_display(&stack.extra_data);
        Some(AnvilInput {
            stack,
            entry,
            display,
        })
    }

    /// The name the target item shows: its custom name, else its localized name.
    fn anvil_target_name(&self, input: &AnvilInput) -> String {
        input.display.name.as_deref().map_or_else(
            || self.localized_item_name(&input.entry.identifier),
            str::to_owned,
        )
    }

    /// Prefills the name box with the target's name whenever a different target is placed,
    /// as the native anvil does, and clears it when the target is taken out.
    pub fn sync_anvil_name(&mut self, player_runtime: &player_state::PlayerState) {
        if self.inventory_ledger(player_runtime).window_kind() != Some(WindowKind::Anvil) {
            return;
        }
        let target = self.anvil_input(player_runtime, ANVIL_TARGET_SLOT);
        let identity = target
            .as_ref()
            .map(|input| (input.stack.network_id, input.stack.nbt_digest));
        if self.screen_state().anvil_target == identity {
            return;
        }
        let name = target
            .map(|input| self.anvil_target_name(&input))
            .unwrap_or_default();
        let state = self.screen_state_mut();
        state.anvil_target = identity;
        state.anvil_name = name;
    }

    /// The result the open anvil makes from its inputs and name box, or `None` when it makes
    /// nothing.
    #[must_use]
    pub fn anvil_preview(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<AnvilPreview> {
        if self.inventory_ledger(player_runtime).window_kind() != Some(WindowKind::Anvil) {
            return None;
        }
        let left = self.anvil_input(player_runtime, ANVIL_TARGET_SLOT)?;
        let right = self.anvil_input(player_runtime, ANVIL_MATERIAL_SLOT);
        let right_item = right.as_ref().map(AnvilInput::item);
        let creative =
            player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative);
        let outcome = inventory::evaluate_anvil(
            &left.item(),
            right_item.as_ref(),
            self.screen_state().anvil_name.trim(),
            &self.anvil_target_name(&left),
            creative,
        )?;
        let extra_data = protocol::edit_item_extra(
            &left.stack.extra_data,
            &protocol::ItemNbtEdit {
                damage: outcome
                    .damage
                    .map(|damage| i32::try_from(damage).unwrap_or(i32::MAX)),
                repair_cost: Some(outcome.repair_cost),
                enchantments: outcome.enchantments.clone(),
                custom_name: outcome.custom_name.clone(),
            },
        )?;
        let mut stack = left.stack;
        stack.stack_network_id = 0;
        stack.nbt_digest = Sha256::digest(&extra_data).into();
        stack.extra_data = extra_data;
        Some(AnvilPreview { stack, outcome })
    }

    /// Whether the player can pay for `preview` now: it is not too expensive, and the player
    /// is in creative or has the levels.
    #[must_use]
    pub fn anvil_affordable(
        &self,
        player_runtime: &player_state::PlayerState,
        preview: &AnvilPreview,
    ) -> bool {
        if preview.outcome.too_expensive {
            return false;
        }
        player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative)
            || self
                .hud
                .experience()
                .is_some_and(|xp| i64::from(xp.level) >= i64::from(preview.outcome.cost))
    }
}

#[cfg(test)]
mod tests;
