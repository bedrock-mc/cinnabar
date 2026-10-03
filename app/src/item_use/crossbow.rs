//! Crossbow's client-owned loaded pose, layered over untouched server stacks.
//!
//! Native 26.50 `releaseUsing` (RVA 09a157e0) stores the selected projectile in
//! `chargedItem` through 02785b90/027960e0, including on duration depletion
//! (09a157a0). `use` (09a13ba0) fires it and removes that compound. Keeping just
//! that state here avoids inventing inventory identities or outgoing NBT.

use protocol::{NetworkItemStack, VerifiedNetworkItemStack};

use super::{AirUse, Needs, UseFrame, classify};
use crate::{mining::FrozenMiningSelection, ui_runtime::UiRuntime};

#[derive(Debug, Clone, PartialEq)]
struct Prediction {
    item: VerifiedNetworkItemStack,
    revision: u64,
    /// `None` is a fired crossbow, not absence of a prediction.
    projectile: Option<&'static str>,
}

#[derive(Debug, Default, Clone)]
pub(super) struct CrossbowPredictions {
    slots: [Option<Prediction>; protocol::HOTBAR_SLOT_COUNT as usize],
}

impl CrossbowPredictions {
    pub(super) fn clear(&mut self) {
        self.slots.fill(None);
    }

    pub(super) fn predict(
        &mut self,
        selection: &FrozenMiningSelection,
        revision: Option<u64>,
        projectile: Option<&'static str>,
    ) {
        if let (Some(slot), Some(revision)) =
            (self.slots.get_mut(usize::from(selection.slot)), revision)
        {
            *slot = Some(Prediction {
                item: selection.item.clone(),
                revision,
                projectile,
            });
        }
    }

    pub(super) fn selected_projectile(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        ui: &UiRuntime,
    ) -> Option<Option<&'static str>> {
        let slot = ui.selected_hotbar_slot(player_runtime)?;
        let stack = ui.selected_stack(player_runtime)?;
        let revision = ui
            .inventory_ledger(player_runtime)
            .authoritative_slot_revision(slot)?;
        self.projectile(slot, revision, |item| matches_stack(item, stack))
    }

    pub(super) fn slot_projectile(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        ui: &UiRuntime,
        slot: u8,
    ) -> Option<Option<&'static str>> {
        let stack = ui.inventory_ledger(player_runtime).displayed_stack(slot)?;
        let revision = ui
            .inventory_ledger(player_runtime)
            .authoritative_slot_revision(slot)?;
        self.projectile(slot, revision, |item| matches_stack(item, stack))
    }

    fn projectile(
        &self,
        slot: u8,
        revision: u64,
        matches: impl FnOnce(&VerifiedNetworkItemStack) -> bool,
    ) -> Option<Option<&'static str>> {
        let prediction = self.slots.get(usize::from(slot))?.as_ref()?;
        (prediction.revision == revision && matches(&prediction.item))
            .then_some(prediction.projectile)
    }

    pub(super) fn verified_projectile(
        &self,
        selection: &FrozenMiningSelection,
        revision: Option<u64>,
    ) -> Option<Option<&'static str>> {
        self.projectile(selection.slot, revision?, |item| item == &selection.item)
    }

    pub(super) fn air_use(&self, frame: &UseFrame) -> Option<AirUse> {
        if !is_crossbow(frame.air_use) {
            return frame.air_use;
        }
        let Some(selection) = &frame.selection else {
            return frame.air_use;
        };
        let Some(projectile) = self.verified_projectile(selection, frame.inventory_revision) else {
            return frame.air_use;
        };
        let quick_charge = protocol::item_enchantment_level(
            selection.item.extra_data(),
            super::QUICK_CHARGE_ENCHANTMENT_ID,
        )
        .unwrap_or(0);
        classify(
            "minecraft:crossbow",
            projectile.is_some(),
            quick_charge,
            None,
        )
    }
}

fn matches_stack(item: &VerifiedNetworkItemStack, stack: &NetworkItemStack) -> bool {
    item.network_id() == stack.network_id
        && item.metadata() == stack.metadata
        && item.stack_network_id() == stack.stack_network_id
        && item.count() == stack.count
        && item.block_runtime_id() == stack.block_runtime_id
        && item.nbt_digest() == stack.nbt_digest
}

pub(super) const fn is_crossbow(air_use: Option<AirUse>) -> bool {
    matches!(
        air_use,
        Some(AirUse::Instant)
            | Some(AirUse::Hold {
                needs: Needs::ArrowOrOffhandRocket,
                ..
            })
    )
}

/// `releaseUsing` checks the offhand for either projectile first, then inventory
/// arrows, and synthesizes an arrow only in creative (09a157e0).
pub(super) fn loading_projectile(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    stream: &client_world::WorldStream,
    ui: &UiRuntime,
    creative: bool,
) -> Option<&'static str> {
    let name = |stack: &NetworkItemStack| {
        (!stack.is_empty())
            .then(|| stream.item_identifier(stack.network_id))
            .flatten()
    };
    if let Some(offhand) = ui.gameplay_hud().offhand_stack().and_then(name) {
        match &*offhand {
            "minecraft:firework_rocket" => return Some("minecraft:firework_rocket"),
            "minecraft:arrow" => return Some("minecraft:arrow"),
            _ => {}
        }
    }
    (creative
        || (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter_map(|slot| ui.inventory_ledger(player_runtime).displayed_stack(slot))
            .filter_map(name)
            .any(|identifier| &*identifier == "minecraft:arrow"))
    .then_some("minecraft:arrow")
}
