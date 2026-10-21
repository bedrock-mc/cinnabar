//! The open grindstone's predicted result, which the server does not preview.

use inventory::inventory_ledger::InventoryTarget;
use protocol::{NetworkItemStack, WindowKind};
use sha2::{Digest, Sha256};

use super::UiRuntime;

/// The grindstone's top and bottom input slots.
const GRINDSTONE_INPUTS: [u8; 2] = [16, 17];

/// The grindstone's predicted result and the fields its take sends.
#[derive(Clone, Debug, PartialEq)]
pub struct GrindstonePreview {
    pub stack: NetworkItemStack,
    /// The server stack id of the input the result copies.
    pub source_stack_id: i32,
    pub repair_cost: i32,
}

impl UiRuntime {
    /// Predicts the open grindstone's result from its inputs.
    pub(super) fn grindstone_preview(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<GrindstonePreview> {
        let ledger = self.inventory_ledger(player_runtime);
        if ledger.window_kind() != Some(WindowKind::Grindstone) {
            return None;
        }
        let facts = ledger.item_facts();
        let stacks = GRINDSTONE_INPUTS.map(|slot| {
            ledger
                .target_stack(InventoryTarget::Craft(slot))
                .filter(|stack| stack.count > 0)
        });
        let data =
            stacks.map(|stack| stack.map(|stack| protocol::item_anvil_data(&stack.extra_data)));
        let mut items = [None, None];
        for (index, (stack, data)) in stacks.iter().zip(&data).enumerate() {
            let (Some(stack), Some(data)) = (stack, data) else {
                continue;
            };
            // Unreadable enchantments cannot be predicted.
            let data = data.as_ref()?;
            let fact = facts.get(stack.network_id);
            items[index] = Some(inventory::GrindstoneItem {
                kind: facts.kind(stack.network_id),
                count: stack.count,
                damage: protocol::item_stack_damage(stack).unwrap_or(0),
                max_damage: fact.durability.maximum(),
                enchantments: &data.enchantments,
                repair_cost: protocol::item_repair_cost(&stack.extra_data),
                has_user_data: !(stack.extra_data.is_empty()
                    || stack.extra_data.as_ref() == [0; 10]),
                trimmed: protocol::item_has_trim(&stack.extra_data),
                enchanted_book: fact
                    .traits
                    .contains(assets::facts::ItemTraits::ENCHANTED_BOOK),
            });
        }
        let outcome = inventory::evaluate_grindstone(items[0].as_ref(), items[1].as_ref())?;
        let source = stacks[outcome.source]?;
        let mut stack = if outcome.plain_book {
            let book = ledger
                .negotiated_item_registry()?
                .values()
                .find(|entry| entry.identifier.as_ref() == "minecraft:book")?;
            NetworkItemStack {
                network_id: book.network_id,
                count: 1,
                ..NetworkItemStack::empty()
            }
        } else {
            let extra_data = protocol::edit_item_extra(
                &source.extra_data,
                &protocol::ItemNbtEdit {
                    damage: outcome
                        .damage
                        .map(|damage| i32::try_from(damage).unwrap_or(i32::MAX)),
                    repair_cost: Some(outcome.repair_cost),
                    enchantments: Some(outcome.enchantments.clone()),
                    ..Default::default()
                },
            )?;
            NetworkItemStack {
                extra_data,
                ..source.clone()
            }
        };
        stack.stack_network_id = 0;
        stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
        Some(GrindstonePreview {
            stack,
            source_stack_id: source.stack_network_id,
            repair_cost: outcome.repair_cost,
        })
    }

    /// The grindstone output shown to the player, preferring a server preview to prediction.
    pub(super) fn grindstone_output(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<NetworkItemStack> {
        self.inventory_ledger(player_runtime)
            .created_output_stack()
            .cloned()
            .or_else(|| {
                self.grindstone_preview(player_runtime)
                    .map(|preview| preview.stack)
            })
    }
}

#[cfg(test)]
mod tests;
