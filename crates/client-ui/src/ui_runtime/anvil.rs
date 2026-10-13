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

/// The target facts whose replacement should prefill the anvil name field again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnvilTargetIdentity {
    network_id: i32,
    metadata: u32,
    nbt_digest: [u8; 32],
    custom_name: Option<std::sync::Arc<str>>,
}

/// One occupied anvil input with data that can be edited without losing item properties.
struct AnvilInput {
    stack: NetworkItemStack,
    entry: ItemRegistryEntry,
    data: protocol::ItemAnvilData,
    damage: Option<u32>,
}

impl AnvilInput {
    /// The durability the registry gives this item: its components' maximum, else the
    /// vanilla maximum for an item the server does not define by components.
    fn max_damage(&self) -> Option<u32> {
        client_world::resolve_item_durability(&self.entry, None).maximum()
    }

    /// The calculator's view of this input.
    fn item(&self) -> inventory::AnvilItem<'_> {
        inventory::AnvilItem {
            identifier: &self.entry.identifier,
            count: self.stack.count,
            damage: self.damage.unwrap_or(0),
            max_damage: self.max_damage(),
            enchantments: &self.data.enchantments,
            repair_cost: protocol::item_repair_cost(&self.stack.extra_data),
            custom_name: self.data.custom_name.as_deref(),
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
        let mut data = protocol::item_anvil_data(&stack.extra_data)?;
        let overlay = ledger.presented_target_overlay(InventoryTarget::Craft(slot));
        if let Some(name) = overlay.and_then(|overlay| overlay.custom_name.clone()) {
            data.custom_name = Some(name);
        }
        let damage = overlay
            .and_then(|overlay| overlay.durability_correction)
            .and_then(|damage| u32::try_from(damage).ok())
            .or_else(|| protocol::item_stack_damage(&stack));
        Some(AnvilInput {
            stack,
            entry,
            data,
            damage,
        })
    }

    /// The name the target item shows: its custom name, else its localized name.
    fn anvil_target_name(&self, input: &AnvilInput) -> String {
        input.data.custom_name.as_deref().map_or_else(
            || {
                super::presentation::inventory_tooltip::default_name(
                    self,
                    &input.entry.identifier,
                    input.stack.metadata,
                )
            },
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
        let identity = target.as_ref().map(|input| AnvilTargetIdentity {
            network_id: input.stack.network_id,
            metadata: input.stack.metadata,
            nbt_digest: input.stack.nbt_digest,
            custom_name: input.data.custom_name.clone(),
        });
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
        let right = if self
            .inventory_ledger(player_runtime)
            .target_stack(InventoryTarget::Craft(ANVIL_MATERIAL_SLOT))
            .is_some_and(|stack| stack.count > 0)
        {
            Some(self.anvil_input(player_runtime, ANVIL_MATERIAL_SLOT)?)
        } else {
            None
        };
        let right_item = right.as_ref().map(AnvilInput::item);
        let creative =
            player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative);
        let outcome = inventory::evaluate_anvil(
            &left.item(),
            right_item.as_ref(),
            &self.screen_state().anvil_name,
            &self.anvil_target_name(&left),
            creative,
        )?;
        let extra_data = protocol::edit_item_extra(
            &left.stack.extra_data,
            &protocol::ItemNbtEdit {
                damage: outcome
                    .damage
                    .or(left.damage)
                    .map(|damage| i32::try_from(damage).unwrap_or(i32::MAX)),
                repair_cost: Some(outcome.repair_cost),
                enchantments: outcome.enchantments.clone(),
                custom_name: outcome.custom_name.clone().or_else(|| {
                    left.data
                        .custom_name
                        .as_ref()
                        .map(|name| Some(name.to_string()))
                }),
            },
        )?;
        let mut stack = left.stack;
        stack.stack_network_id = 0;
        stack.nbt_digest = Sha256::digest(&extra_data).into();
        stack.extra_data = extra_data;
        Some(AnvilPreview { stack, outcome })
    }

    /// The anvil output shown to the player, preferring the server's result to prediction.
    pub(super) fn anvil_output(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<NetworkItemStack> {
        self.inventory_ledger(player_runtime)
            .created_output_stack()
            .cloned()
            .or_else(|| {
                self.anvil_preview(player_runtime)
                    .map(|preview| preview.stack)
            })
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
