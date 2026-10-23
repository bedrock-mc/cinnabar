//! Selected, hotbar and worn item visuals from one inventory snapshot.

use super::item_icons::{ItemIconFrames, ItemResolver};
use crate::ui_runtime::presentation;
use std::sync::Arc;
use ui::IconRef;

/// The selected slot and stack stay paired across hotbar and held-item capture.
pub(super) struct SelectedItem<'a> {
    pub slot: Option<u8>,
    pub stack: Option<&'a protocol::NetworkItemStack>,
}

impl<'a> SelectedItem<'a> {
    /// Borrows the selected stack from the player's current ledger snapshot.
    pub fn capture(player_runtime: &'a player_state::PlayerState) -> Self {
        let selected_snapshot = player_runtime.selected_stack_snapshot();
        let selected_slot = selected_snapshot.map(|snapshot| snapshot.slot);
        let selected_stack = selected_snapshot.and_then(|snapshot| match snapshot.state {
            inventory::inventory_ledger::PlayerInventorySlot::Present(stack) => Some(stack),
            inventory::inventory_ledger::PlayerInventorySlot::Unknown
            | inventory::inventory_ledger::PlayerInventorySlot::Empty => None,
        });
        Self {
            slot: selected_slot,
            stack: selected_stack,
        }
    }

    /// The identity observed by the selected-name hold timer.
    pub fn identity(&self) -> Option<(u8, i32, u32)> {
        self.stack
            .zip(self.slot)
            .map(|(stack, slot)| (slot, stack.network_id, stack.metadata))
    }
}

/// The displayed hotbar and the diagnostic identities captured beside it.
pub(super) struct HotbarCapture {
    pub durability: [Option<f32>; 9],
    pub icons: [Option<IconRef>; 9],
    pub stacks: [Option<protocol::NetworkItemStack>; 9],
    pub logged: [Option<(Arc<str>, bool)>; 9],
}

/// Uses the selected snapshot for its cell and presented ledger overlays for the whole row.
pub(super) fn hotbar(
    ledger: &inventory::inventory_ledger::PlayerInventoryLedger,
    selected: &SelectedItem<'_>,
    items: &ItemResolver<'_>,
    icon_frames: &ItemIconFrames,
) -> HotbarCapture {
    let runtime = items.runtime;
    let presentation = items.presentation;
    let mut hotbar_durability = [None; 9];
    let mut hotbar_icons = [None; 9];
    let mut hotbar_stacks: [Option<protocol::NetworkItemStack>; 9] = Default::default();
    let mut logged_hotbar: [Option<(Arc<str>, bool)>; 9] = Default::default();
    for (slot, durability) in hotbar_durability.iter_mut().enumerate() {
        let slot = slot as u8;
        let stack = if selected.slot == Some(slot) {
            selected.stack
        } else {
            ledger.displayed_stack(slot)
        };
        if let Some(stack) = stack {
            let item = items.item(stack);
            let identifier = item.as_ref().and_then(|item| item.identifier.clone());
            // Every hotbar cell — selected or not — derives from one
            // ledger-snapshot authority revision: the travelling predicted
            // overlay beside its predicted stack while a gesture is in
            // flight, otherwise the committed overlay. One accepted sparse
            // response therefore refreshes the whole presented row at once.
            let overlay = ledger.presented_slot_overlay(slot);
            *durability = items.durability(
                stack,
                overlay.and_then(|overlay| overlay.durability_correction),
            );
            hotbar_icons[usize::from(slot)] = item.and_then(|item| {
                super::item_icons::stack_icon(
                    runtime,
                    presentation,
                    stack,
                    item,
                    icon_frames.slot(slot),
                )
            });
            hotbar_stacks[usize::from(slot)] = Some(stack.clone());
            logged_hotbar[usize::from(slot)] = Some((
                identifier
                    .unwrap_or_else(|| Arc::from(format!("<network id {}>", stack.network_id))),
                hotbar_icons[usize::from(slot)].is_some(),
            ));
        }
    }
    HotbarCapture {
        durability: hotbar_durability,
        icons: hotbar_icons,
        stacks: hotbar_stacks,
        logged: logged_hotbar,
    }
}

/// Equipment artwork and selected-name facts resolved after the hotbar diagnostic observation.
pub(super) struct EquipmentCapture {
    pub offhand_durability: Option<f32>,
    pub offhand_icon: Option<IconRef>,
    pub armor_icons: [Option<IconRef>; 4],
    pub armor_durability: [Option<f32>; 4],
    pub held_item_icon: Option<IconRef>,
    pub selected_item_name: Option<Arc<str>>,
    pub holding_filled_map: bool,
}

/// Captures offhand, armor and held-item artwork in their publication order.
pub(super) fn equipment(
    player_runtime: &player_state::PlayerState,
    selected: &SelectedItem<'_>,
    items: &ItemResolver<'_>,
    icon_frames: &ItemIconFrames,
) -> EquipmentCapture {
    let runtime = items.runtime;
    let offhand = runtime.local_offhand_stack(player_runtime);
    let offhand_durability = offhand.and_then(|stack| items.durability(stack, None));
    let offhand_icon = offhand.and_then(|stack| items.icon(stack, None));
    let (armor_icons, armor_durability) = {
        let armor = runtime.local_armor(player_runtime);
        let worn = [
            &armor.helmet,
            &armor.chestplate,
            &armor.leggings,
            &armor.boots,
        ];
        (
            worn.map(|stack| items.icon(stack, None)),
            worn.map(|stack| items.durability(stack, None)),
        )
    };
    let held_item_icon = selected
        .stack
        .and_then(|stack| items.icon(stack, selected.slot.and_then(|slot| icon_frames.slot(slot))));
    let selected_item_name = selected.stack.and_then(|stack| {
        let identifier = items.identifier(stack);
        let stated_name = player_runtime.selected_stack_custom_name();
        let facts = runtime.stack_facts(stack);
        presentation::inventory_tooltip::name_line(
            runtime,
            identifier.as_deref(),
            stack.metadata,
            stated_name.as_deref(),
            &facts.display,
        )
        .map(|line| Arc::from(line.text))
    });
    let holding_filled_map = selected
        .stack
        .and_then(|stack| items.identifier(stack))
        .is_some_and(|id| &*id == assets::facts::FILLED_MAP_IDENTIFIER);
    EquipmentCapture {
        offhand_durability,
        offhand_icon,
        armor_icons,
        armor_durability,
        held_item_icon,
        selected_item_name,
        holding_filled_map,
    }
}
