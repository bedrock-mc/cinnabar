//! Crossbow's client-owned loaded pose, layered over untouched server stacks.
//!
//! Vanilla stores the selected projectile in `chargedItem` when charging ends,
//! including when the use duration runs out. Using the loaded crossbow fires it
//! and removes that compound. Keeping just that state here avoids inventing
//! inventory identities or outgoing NBT.

use protocol::{NetworkItemStack, VerifiedNetworkItemStack};

use super::{AirUse, Needs, UseFrame, classify};
use crate::mining::FrozenMiningSelection;

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

    /// Looks up the prediction without borrowing application inventory adapters.
    pub(super) fn projectile_for_stack(
        &self,
        slot: u8,
        revision: u64,
        stack: &NetworkItemStack,
    ) -> Option<Option<&'static str>> {
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
