//! The open grindstone's predicted result, which the server does not preview.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use inventory::inventory_ledger::InventoryTarget;
use protocol::{ItemRegistryEntry, NetworkItemStack, WindowKind};
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

/// Whether user data opens with a compound. Read from the marker, not the envelope length:
/// a shield's blocking tick trails an otherwise empty envelope.
fn has_compound(extra_data: &[u8]) -> bool {
    extra_data.get(..2) == Some(&[0xff, 0xff][..])
}

/// The inputs and registry snapshot one retained prediction was computed from.
#[derive(Debug)]
struct Cached {
    inputs: [Option<NetworkItemStack>; 2],
    registry: Option<Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    preview: Option<GrindstonePreview>,
}

/// Keeps the last grindstone prediction, including one that makes nothing.
#[derive(Debug, Default)]
pub(crate) struct PreviewCache(Mutex<Option<Cached>>);

impl Clone for PreviewCache {
    /// Cloned runtimes recompute on first use.
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl UiRuntime {
    /// The open grindstone's predicted result, recomputed only when its inputs or the item
    /// registry change.
    pub(super) fn grindstone_preview(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<GrindstonePreview> {
        let ledger = self.inventory_ledger(player_runtime);
        let mut cache = self
            .grindstone_cache
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ledger.window_kind() != Some(WindowKind::Grindstone) {
            *cache = None;
            return None;
        }
        let inputs = GRINDSTONE_INPUTS.map(|slot| {
            ledger
                .target_stack(InventoryTarget::Craft(slot))
                .filter(|stack| stack.count > 0)
                .cloned()
        });
        let registry = ledger.item_registry_snapshot();
        if let Some(cached) = cache.as_ref()
            && cached.inputs == inputs
            && match (cached.registry.as_ref(), registry) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
        {
            return cached.preview.clone();
        }
        let preview = self.compute_grindstone_preview(player_runtime);
        *cache = Some(Cached {
            inputs,
            registry: registry.cloned(),
            preview: preview.clone(),
        });
        preview
    }

    /// Derives the grindstone's result from its inputs.
    fn compute_grindstone_preview(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<GrindstonePreview> {
        let ledger = self.inventory_ledger(player_runtime);
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
                // Unbreakable items cannot be merged for durability.
                max_damage: match fact.durability {
                    assets::item::durability::ItemDurability::Maximum { maximum } => Some(maximum),
                    _ => None,
                }
                .filter(|_| !protocol::item_extra_unbreakable(&stack.extra_data)),
                enchantments: &data.enchantments,
                repair_cost: protocol::item_repair_cost(&stack.extra_data),
                has_user_data: has_compound(&stack.extra_data),
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
