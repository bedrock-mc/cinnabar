//! Inventory cells projected from a borrowed ledger and caller-resolved item facts.

use inventory::inventory_ledger::{InventoryTarget, PlayerInventoryLedger};
use protocol::NetworkItemStack;
use std::{collections::HashMap, sync::Arc};
use ui::IconRef;

pub mod cache;

#[derive(Clone, Debug, PartialEq)]
pub struct InventoryIcons(pub [Option<IconRef>; 36]);

impl Default for InventoryIcons {
    fn default() -> Self {
        Self([None; 36])
    }
}

/// The active crafting grid's icons in row-major order plus the previewed
/// output of its unique recipe.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CraftingFrame {
    pub icons: [Option<IconRef>; 9],
    pub output: Option<(Option<IconRef>, protocol::NetworkItemStack)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StorageIcons(pub [Option<IconRef>; protocol::MAX_GENERIC_STORAGE_SLOTS]);

impl Default for StorageIcons {
    fn default() -> Self {
        Self([None; protocol::MAX_GENERIC_STORAGE_SLOTS])
    }
}

/// Durability bar fractions per inventory surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Durability {
    pub player: [Option<f32>; 36],
    pub storage: [Option<f32>; protocol::MAX_GENERIC_STORAGE_SLOTS],
    pub ui: [Option<f32>; protocol::UI_SLOT_COUNT],
}

impl Default for Durability {
    fn default() -> Self {
        Self {
            player: [None; 36],
            storage: [None; protocol::MAX_GENERIC_STORAGE_SLOTS],
            ui: [None; protocol::UI_SLOT_COUNT],
        }
    }
}

/// Slot artwork and hover names captured from the displayed inventory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventoryContents {
    pub inventory_icons: InventoryIcons,
    pub storage_icons: StorageIcons,
    pub crafting: CraftingFrame,
    pub item_names: HashMap<(i32, u32), Arc<str>>,
}

/// Projects player, storage and crafting cells in their display order.
/// Resolvers read item facts without granting screens runtime authority.
pub fn capture_contents(
    ledger: &PlayerInventoryLedger,
    inventory_open: bool,
    crafting_match: impl FnOnce() -> inventory::CraftGridMatch,
    mut resolve_icon: impl FnMut(&NetworkItemStack, Option<u8>) -> Option<IconRef>,
    mut item_name: impl FnMut(&NetworkItemStack) -> Option<String>,
) -> InventoryContents {
    let mut inventory_icons = InventoryIcons::default();
    for (slot, icon) in inventory_icons.0.iter_mut().enumerate() {
        if let Some(stack) = ledger.displayed_stack(slot as u8) {
            *icon = resolve_icon(stack, Some(slot as u8));
        }
    }
    let mut storage_icons = StorageIcons::default();
    for (slot, icon) in storage_icons.0.iter_mut().enumerate() {
        if let Some(stack) = ledger.furnace_visual_stack(slot as u8) {
            *icon = resolve_icon(stack, None);
        }
    }
    let mut crafting = CraftingFrame::default();
    if inventory_open {
        for (icon, slot) in crafting
            .icons
            .iter_mut()
            .zip(ledger.crafting_grid().slots())
        {
            let target = inventory::inventory_ledger::InventoryTarget::Craft(slot);
            if let Some(stack) = ledger.target_stack(target) {
                *icon = resolve_icon(stack, None);
            }
        }
        if let inventory::CraftGridMatch::Unique(recipe) = crafting_match() {
            let output = recipe.output();
            let stack = protocol::NetworkItemStack {
                network_id: output.network_id,
                metadata: u32::from(output.aux),
                count: u16::from(output.count),
                block_runtime_id: output.block_runtime_id as i32,
                ..protocol::NetworkItemStack::empty()
            };
            let icon = resolve_icon(&stack, None);
            crafting.output = Some((icon, stack));
        }
    }
    // Hover names for the open container's cells (JSON-UI tooltips).
    let item_names = if inventory_open {
        (0..36u8)
            .filter_map(|slot| ledger.displayed_stack(slot))
            .chain(
                (0..protocol::MAX_GENERIC_STORAGE_SLOTS)
                    .filter_map(|slot| ledger.furnace_visual_stack(slot as u8)),
            )
            .filter_map(|stack| {
                let name = item_name(stack)?;
                Some(((stack.network_id, stack.metadata), Arc::from(name)))
            })
            .collect()
    } else {
        Default::default()
    };
    InventoryContents {
        inventory_icons,
        storage_icons,
        crafting,
        item_names,
    }
}

/// Captures durability corrections and UI-slot icons after inventory artwork.
pub fn capture_decorations(
    ledger: &PlayerInventoryLedger,
    mut fraction: impl FnMut(&NetworkItemStack, Option<i32>) -> Option<f32>,
    mut resolve_icon: impl FnMut(&NetworkItemStack) -> Option<IconRef>,
) -> (Durability, [Option<IconRef>; protocol::UI_SLOT_COUNT]) {
    let mut durability = Durability::default();
    let mut ui_icons = [None; protocol::UI_SLOT_COUNT];
    for (slot, bar) in durability.player.iter_mut().enumerate() {
        if let Some(stack) = ledger.displayed_stack(slot as u8) {
            let correction = ledger
                .presented_slot_overlay(slot as u8)
                .and_then(|overlay| overlay.durability_correction);
            *bar = fraction(stack, correction);
        }
    }
    for (slot, bar) in durability.storage.iter_mut().enumerate() {
        if let Some(stack) = ledger.storage_stack(slot as u8) {
            *bar = fraction(stack, None);
        }
    }
    for slot in 0..protocol::UI_SLOT_COUNT as u8 {
        let stack = if slot == protocol::CREATED_OUTPUT_SLOT {
            ledger.created_output_stack()
        } else if protocol::ui_slot_container_name(slot).is_some() {
            ledger.target_stack(InventoryTarget::Craft(slot))
        } else {
            None
        };
        if let Some(stack) = stack {
            durability.ui[usize::from(slot)] = fraction(stack, None);
            ui_icons[usize::from(slot)] = resolve_icon(stack);
        }
    }
    (durability, ui_icons)
}

#[cfg(test)]
mod tests;
