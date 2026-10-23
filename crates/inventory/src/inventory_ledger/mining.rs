//! Committed tool wear and the legacy response history retained for client block breaks.

use super::cells::Held;
use super::queue::{CellPrediction, RollbackAuthority};
use super::{Cell, PlayerInventoryLedger};

impl PlayerInventoryLedger {
    /// Commits an admitted destroy's exact slot change, retaining its response and rollback.
    /// A replaced or already committed source cannot be applied a second time.
    pub fn settle_destroy_slot_change(
        &mut self,
        slot: u8,
        change: &protocol::PredictedSlotChange,
    ) -> bool {
        let cell = Cell::Inventory(slot);
        if slot >= protocol::HOTBAR_SLOT_COUNT
            || !protocol::is_legacy_request_id(change.legacy_request_id)
        {
            return false;
        }
        let Some(before) = self.confirmed.get(cell).cloned() else {
            return false;
        };
        let Ok(from) = protocol::VerifiedNetworkItemStack::try_new(
            before.stack.clone(),
            before.stack.nbt_digest,
        ) else {
            return false;
        };
        if from != change.from
            || (!change.to.is_empty()
                && (change.to.network_id() != from.network_id()
                    || change.to.metadata() != from.metadata()
                    || change.to.block_runtime_id() != from.block_runtime_id()
                    || change.to.stack_network_id() != change.legacy_request_id))
        {
            return false;
        }
        let damage = protocol::item_extra_damage_integer(change.to.extra_data()).unwrap_or(0);
        let Some(request_id) = self.enqueue_mining(slot, damage, Some(change.legacy_request_id))
        else {
            return false;
        };
        let after = (!change.to.is_empty()).then(|| {
            let mut held: Held = before.clone();
            held.stack.count = change.to.count();
            held.stack.stack_network_id = change.to.stack_network_id();
            held.stack.extra_data = change.to.extra_data().into();
            held.stack.nbt_digest = change.to.nbt_digest();
            held.overlay
                .get_or_insert_with(Default::default)
                .durability_correction = Some(damage);
            held
        });
        self.confirmed.set(cell, after.clone());
        self.rebase_predictions(
            i32::MAX,
            cell,
            Some(&before),
            after.clone(),
            RollbackAuthority::Known,
        );
        let request = self
            .queue
            .iter_mut()
            .find(|request| request.request_id == request_id)
            .expect("new mining request");
        request.predicted.push(CellPrediction {
            cell,
            before: Some(before),
            rollback: RollbackAuthority::Known,
            held: after,
            active: false,
            previous_owner: None,
            current: true,
            owns_empty_backing: false,
        });
        self.refold();
        true
    }
}
