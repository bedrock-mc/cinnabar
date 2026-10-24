//! The open anvil's predicted result and cost, and the name box it prefills.

use inventory::inventory_ledger::{ANVIL_MATERIAL_SLOT, ANVIL_TARGET_SLOT, InventoryTarget};
use protocol::{NetworkItemStack, WindowKind};
use sha2::{Digest, Sha256};

use super::UiRuntime;

pub(super) mod cache;

/// The anvil's predicted result stack and what taking it costs.
#[derive(Clone, Debug, PartialEq)]
pub struct AnvilPreview {
    pub stack: NetworkItemStack,
    pub outcome: inventory::AnvilOutcome,
    /// The name the take sends; the prediction applied the same text.
    pub take_name: Option<std::sync::Arc<str>>,
}

/// The target and resolved name last used to prefill the editable field.
#[derive(Clone, Debug)]
pub struct AnvilNameState {
    target: AnvilTargetIdentity,
    /// Invalidates name resolution when the response source changes, without changing identity.
    stated_name: Option<std::sync::Arc<str>>,
    names_generation: u64,
    prefilled: String,
}

/// Cheap target facts that change when a different item is placed.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AnvilTargetIdentity {
    network_id: i32,
    stack_network_id: i32,
    metadata: u32,
    nbt_digest: [u8; 32],
    identifier: Option<std::sync::Arc<str>>,
}

impl AnvilTargetIdentity {
    /// Mutable item data and an acknowledged prediction retain the placed target.
    fn same_item(&self, other: &Self, acknowledged: bool) -> bool {
        self.network_id == other.network_id
            && (self.stack_network_id == other.stack_network_id || acknowledged)
            && self.metadata == other.metadata
            && self.identifier == other.identifier
    }
}

/// One occupied anvil input with data that can be edited without losing item properties.
struct AnvilInput {
    stack: NetworkItemStack,
    fact: assets::facts::ItemFact,
    kind: inventory::SessionItemKind,
    retail_id: Option<i32>,
    data: protocol::ItemAnvilData,
    damage: Option<u32>,
}

impl AnvilInput {
    /// Returns the durability admitted for this negotiated item.
    fn max_damage(&self) -> Option<u32> {
        self.fact.durability.maximum()
    }

    /// The calculator's view of this input.
    fn item(&self) -> inventory::AnvilItem<'_> {
        inventory::AnvilItem {
            kind: Some(self.kind),
            retail_id: self.retail_id,
            fact: self.fact,
            count: self.stack.count,
            damage: self.damage.unwrap_or(0),
            max_damage: self.max_damage(),
            enchantments: &self.data.enchantments,
            repair_cost: protocol::item_repair_cost(&self.stack.extra_data),
            custom_name: self.data.custom_name.as_deref(),
        }
    }
}

/// Preserves unchanged names and materializes an overlay pair only when it differs.
/// An actual rename lets the item editor retire the obsolete filtered name.
fn anvil_name_edit(
    extra_data: &[u8],
    overlay: Option<&inventory::inventory_ledger::StackResponseOverlay>,
    rename: Option<Option<String>>,
) -> protocol::ItemNbtEdit {
    if let Some(name) = rename {
        return protocol::ItemNbtEdit {
            custom_name: Some(name),
            ..Default::default()
        };
    }
    let Some(overlay) = overlay else {
        return Default::default();
    };
    let Some(name) = overlay.custom_name.as_deref() else {
        return Default::default();
    };
    let filtered = overlay
        .filtered_custom_name
        .as_deref()
        .filter(|name| !name.is_empty());
    if protocol::item_custom_name(extra_data).as_deref() == Some(name)
        && protocol::item_filtered_custom_name(extra_data).as_deref() == filtered
    {
        return Default::default();
    }
    protocol::ItemNbtEdit {
        custom_name: Some(Some(name.to_owned())),
        filtered_custom_name: Some(filtered.map(str::to_owned)),
        ..Default::default()
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
            kind: ledger.item_facts().kind(stack.network_id)?,
            fact: *ledger.item_facts().get(stack.network_id),
            retail_id: ledger.item_facts().retail_id(stack.network_id),
            stack,
            data,
            damage,
        })
    }

    /// The target's custom name, preferring a retained server correction to its NBT.
    fn anvil_target_custom_name(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<std::sync::Arc<str>> {
        let ledger = self.inventory_ledger(player_runtime);
        let target = InventoryTarget::Craft(ANVIL_TARGET_SLOT);
        let stack = ledger
            .target_stack(target)
            .filter(|stack| stack.count > 0)?;
        ledger
            .presented_target_overlay(target)
            .and_then(|overlay| overlay.custom_name.clone())
            .or_else(|| protocol::item_custom_name(&stack.extra_data))
    }

    /// The name the target item shows: its custom name, else its localized name.
    fn anvil_target_name(&self, player_runtime: &player_state::PlayerState) -> Option<String> {
        self.anvil_target_custom_name(player_runtime)
            .map(|name| name.to_string())
            .or_else(|| {
                let ledger = self.inventory_ledger(player_runtime);
                let stack = ledger
                    .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
                    .filter(|stack| stack.count > 0)?;
                Some(super::presentation::inventory_tooltip::default_name(
                    self,
                    &ledger.negotiated_item_entry(stack.network_id)?.identifier,
                    stack.metadata,
                ))
            })
    }

    /// The name a take of a `network_id`/`metadata` result sends, which the prediction also
    /// applies: the name-box text, or `None` for the result's default name. The server reads a
    /// missing name like empty text, which removes a custom name.
    pub(super) fn anvil_take_name(
        &self,
        player_runtime: &player_state::PlayerState,
        network_id: i32,
        metadata: u32,
    ) -> Option<std::sync::Arc<str>> {
        let name = self.screen_state().anvil_name.as_str();
        let default = self
            .inventory_ledger(player_runtime)
            .negotiated_item_entry(network_id)
            .map(|entry| {
                super::presentation::inventory_tooltip::default_name(
                    self,
                    &entry.identifier,
                    metadata,
                )
            });
        (default.as_deref() != Some(name)).then(|| std::sync::Arc::from(name))
    }

    /// Prefills the name box with the target's name whenever a different target is placed,
    /// as the native anvil does, and clears it when the target is taken out.
    pub fn sync_anvil_name(&mut self, player_runtime: &player_state::PlayerState) {
        if self.inventory_ledger(player_runtime).window_kind() != Some(WindowKind::Anvil) {
            return;
        }
        let ledger = self.inventory_ledger(player_runtime);
        let target = InventoryTarget::Craft(ANVIL_TARGET_SLOT);
        let identity = ledger
            .target_stack(target)
            .filter(|stack| stack.count > 0)
            .map(|stack| AnvilTargetIdentity {
                network_id: stack.network_id,
                stack_network_id: stack.stack_network_id,
                metadata: stack.metadata,
                nbt_digest: stack.nbt_digest,
                identifier: ledger
                    .negotiated_item_entry(stack.network_id)
                    .map(|entry| entry.identifier.clone()),
            });
        let Some(identity) = identity else {
            let state = self.screen_state_mut();
            state.anvil_target = None;
            state.anvil_name.clear();
            return;
        };
        let stated_name = ledger
            .presented_target_overlay(target)
            .and_then(|overlay| overlay.custom_name.clone());
        let names_generation = self.item_name_generation();
        let state = self.screen_state();
        let previous = state.anvil_target.as_ref();
        let same_source = previous.is_some_and(|previous| previous.stated_name == stated_name);
        let same_facts =
            same_source && previous.is_some_and(|previous| previous.target == identity);
        let same_target = previous.is_some_and(|previous| {
            previous.target.same_item(
                &identity,
                ledger.target_retains_stack_id(target, previous.target.stack_network_id),
            )
        });
        if same_facts
            && previous.is_some_and(|previous| previous.names_generation == names_generation)
        {
            return;
        }
        let untouched = previous.is_some_and(|previous| state.anvil_name == previous.prefilled);
        let name = self.anvil_target_name(player_runtime).unwrap_or_default();
        let changed_target_name =
            !same_facts && previous.is_some_and(|previous| previous.prefilled != name);
        let state = self.screen_state_mut();
        state.anvil_target = Some(AnvilNameState {
            target: identity,
            stated_name,
            names_generation,
            prefilled: name.clone(),
        });
        if !same_target || untouched || changed_target_name {
            state.anvil_name = name;
        }
    }

    /// Rebuilds the prediction after an input, name or calculation rule changes.
    fn compute_anvil_preview(
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
        let current_name = self.anvil_target_name(player_runtime)?;
        // A plain book enchanted from a book becomes the enchanted book, whose default name
        // decides what the take sends.
        let ledger = self.inventory_ledger(player_runtime);
        let book_result = left.fact.traits.contains(assets::facts::ItemTraits::BOOK)
            && !left
                .fact
                .traits
                .contains(assets::facts::ItemTraits::ENCHANTED_BOOK)
            && right.as_ref().is_some_and(|right| {
                right
                    .fact
                    .traits
                    .contains(assets::facts::ItemTraits::ENCHANTED_BOOK)
            });
        let (result_id, result_metadata) = match ledger.item_facts().enchanted_book_id() {
            Some(book) if book_result => (book, 0),
            _ => (left.stack.network_id, left.stack.metadata),
        };
        let take_name = self.anvil_take_name(player_runtime, result_id, result_metadata);
        let outcome = inventory::evaluate_anvil(
            &left.item(),
            right_item.as_ref(),
            take_name.as_deref().unwrap_or(""),
            &current_name,
            creative,
        )?;
        let names = anvil_name_edit(
            &left.stack.extra_data,
            self.inventory_ledger(player_runtime)
                .presented_target_overlay(InventoryTarget::Craft(ANVIL_TARGET_SLOT)),
            outcome.custom_name.clone(),
        );
        let extra_data = protocol::edit_item_extra(
            &left.stack.extra_data,
            &protocol::ItemNbtEdit {
                damage: outcome
                    .damage
                    .or(left.damage)
                    .map(|damage| i32::try_from(damage).unwrap_or(i32::MAX)),
                repair_cost: Some(outcome.repair_cost),
                enchantments: outcome.enchantments.clone(),
                ..names
            },
        )?;
        let mut stack = left.stack;
        if outcome.enchant_book {
            stack.network_id = self
                .inventory_ledger(player_runtime)
                .item_facts()
                .enchanted_book_id()?;
            stack.count = 1;
            stack.metadata = 0;
            stack.block_runtime_id = 0;
        }
        stack.stack_network_id = 0;
        stack.nbt_digest = Sha256::digest(&extra_data).into();
        stack.extra_data = extra_data;
        Some(AnvilPreview {
            stack,
            outcome,
            take_name,
        })
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
                    .filter(|preview| !preview.outcome.too_expensive)
                    .map(|preview| preview.stack.clone())
            })
    }
}

#[cfg(test)]
mod tests;
