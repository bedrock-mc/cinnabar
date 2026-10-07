//! Held-placement inventory prediction and ordered outbound packets.
use crate::mining::FrozenMiningSelection;
use protocol::PredictedSlotChange;

/// Keeps local count changes until the server restates the selected slot.
#[derive(Debug, Default)]
pub struct HeldPlacementInventory {
    slots: [Option<PlacementStack>; protocol::HOTBAR_SLOT_COUNT as usize],
}

#[derive(Debug)]
struct PlacementStack {
    server: FrozenMiningSelection,
    revision: u64,
    item: protocol::VerifiedNetworkItemStack,
}

impl HeldPlacementInventory {
    /// Reuses a prediction only while the authoritative slot and its write revision match.
    pub fn selection(
        &mut self,
        server: &FrozenMiningSelection,
        revision: u64,
    ) -> FrozenMiningSelection {
        if let Some(selection) = self.predicted_selection(server, revision) {
            return selection;
        }
        if let Some(slot) = self.slots.get_mut(usize::from(server.slot)) {
            *slot = None;
        }
        server.clone()
    }

    /// Borrows the admitted count prediction for another use on the same authoritative slot.
    pub fn predicted_selection(
        &self,
        server: &FrozenMiningSelection,
        revision: u64,
    ) -> Option<FrozenMiningSelection> {
        let predicted = self.slots.get(usize::from(server.slot))?.as_ref()?;
        (predicted.server == *server && predicted.revision == revision).then(|| {
            FrozenMiningSelection {
                slot: server.slot,
                item: predicted.item.clone(),
            }
        })
    }

    /// Builds a placement delta without committing it before transport accepts the transaction.
    pub fn prepare_change(
        &self,
        selected: &FrozenMiningSelection,
        legacy_request_id: i32,
    ) -> PredictedSlotChange {
        PredictedSlotChange {
            legacy_request_id,
            from: selected.item.clone(),
            to: selected.item.less_one(legacy_request_id),
        }
    }

    /// Publishes an admitted count change against the slot observation that produced it.
    pub fn commit(
        &mut self,
        server: FrozenMiningSelection,
        revision: u64,
        change: PredictedSlotChange,
    ) {
        if let Some(slot) = self.slots.get_mut(usize::from(server.slot)) {
            *slot = Some(PlacementStack {
                server,
                revision,
                item: change.to,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{NetworkItemStack, VerifiedNetworkItemStack};

    /// A verified block stack in the selected hotbar slot.
    fn selected(count: u16) -> FrozenMiningSelection {
        let mut stack = NetworkItemStack::empty();
        stack.network_id = 1;
        stack.stack_network_id = 41;
        stack.count = count;
        stack.block_runtime_id = 77;
        let digest = stack.nbt_digest;
        FrozenMiningSelection {
            slot: 2,
            item: VerifiedNetworkItemStack::try_new(stack, digest).unwrap(),
        }
    }

    #[test]
    fn held_placement_counts_each_admitted_repeat_and_rebases_on_server_write() {
        let server = selected(3);
        let mut inventory = HeldPlacementInventory::default();
        let first = inventory.selection(&server, 1);
        let refused = inventory.prepare_change(&first, -4);
        assert_eq!(inventory.selection(&server, 1).item.count(), 3);
        inventory.commit(server.clone(), 1, refused);
        let second = inventory.selection(&server, 1);
        assert_eq!(second.item.count(), 2);
        let change = inventory.prepare_change(&second, -6);
        assert_eq!(change.from.count(), 2);
        assert_eq!(change.to.count(), 1);
        inventory.commit(server.clone(), 1, change);
        assert_eq!(inventory.selection(&server, 1).item.count(), 1);
        assert_eq!(inventory.selection(&server, 2).item.count(), 3);
    }

    #[test]
    fn reselecting_slots_preserves_each_admitted_prediction_until_its_server_write() {
        let first = selected(3);
        let mut second = selected(5);
        second.slot = first.slot + 1;
        let mut inventory = HeldPlacementInventory::default();
        let change = inventory.prepare_change(&first, -4);
        let expected_first = change.to.clone();
        inventory.commit(first.clone(), 1, change);
        let selected_second = inventory.selection(&second, 1);
        let change = inventory.prepare_change(&selected_second, -6);
        let expected_second = change.to.clone();
        inventory.commit(second.clone(), 1, change);
        assert_eq!(inventory.selection(&first, 1).item, expected_first);
        assert_eq!(inventory.selection(&second, 1).item, expected_second);
        assert_eq!(inventory.selection(&first, 2).item, first.item);
        assert_eq!(inventory.selection(&second, 1).item, expected_second);
        let mut invalid = first.clone();
        invalid.slot = protocol::HOTBAR_SLOT_COUNT;
        assert_eq!(inventory.selection(&invalid, 1), invalid);
        let ignored = inventory.prepare_change(&invalid, -8);
        inventory.commit(invalid, 1, ignored);
        assert_eq!(inventory.selection(&second, 1).item, expected_second);
    }

    #[test]
    fn position_correction_preserves_admitted_inventory_until_slot_write_or_new_session() {
        let server = selected(3);
        let mut runtime = crate::block_use::BlockUseRuntime::default();
        runtime.synchronize((7, 0));
        let change = runtime.inventory.prepare_change(&server, -4);
        let expected = change.to.clone();
        runtime.inventory.commit(server.clone(), 1, change);
        runtime.synchronize((7, 1));
        let corrected = runtime.inventory.selection(&server, 1);
        let next = runtime.inventory.prepare_change(&corrected, -6);
        assert_eq!(next.from, expected);
        assert_eq!(next.to.count(), 1);
        assert_eq!(runtime.inventory.selection(&server, 2).item, server.item);
        runtime.inventory.commit(server.clone(), 2, next);
        runtime.synchronize((8, 0));
        assert_eq!(runtime.inventory.selection(&server, 2).item, server.item);
    }

    #[test]
    fn last_placement_empties_the_selected_stack_until_correction() {
        let server = selected(1);
        let mut inventory = HeldPlacementInventory::default();
        let change = inventory.prepare_change(&server, -4);
        assert!(change.to.is_empty());
        inventory.commit(server.clone(), 1, change);
        assert!(inventory.selection(&server, 1).item.is_empty());
        assert!(!inventory.selection(&server, 2).item.is_empty());
    }
    #[test]
    fn placement_press_uses_remaining_stack_for_one_air_fallback() {
        let server = selected(3);
        let mut inventory = HeldPlacementInventory::default();
        let mut air = crate::item_use::ItemUseRuntime::default();
        let id = air.next_legacy_request_id();
        let change = inventory.prepare_change(&server, id);
        inventory.commit(server.clone(), 1, change);
        let selection = inventory.predicted_selection(&server, 1).unwrap();
        assert_eq!(selection.item.count(), 2);
        assert!(inventory.predicted_selection(&server, 2).is_none());
        let mut frame = crate::item_use::UseFrame {
            tick: 1,
            now_millis: 1_000,
            position: [0.5, 65.62, 0.5],
            held: true,
            selection: Some(selection),
            air_use: None,
            ready: false,
            creative: false,
            inventory_revision: Some(1),
            charge_projectile: None,
            press_consumed: false,
        };
        air.observe_press(true);
        let outcome = air.step(&frame);
        assert_eq!(outcome.packets.len(), 1);
        let protocol::wire::valentine::bedrock::version::v1_26_51::McpePacketData::InventoryTransactionPacket(
            packet,
        ) = &outcome.packets[0].data
        else {
            panic!("air fallback");
        };
        assert_eq!(packet.legacy_request_id.id, 0);
        let protocol::wire::valentine::bedrock::version::v1_26_51::InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction) = &packet.transaction else {
            panic!("item use");
        };
        assert_eq!(transaction.item.stacksize, 2);
        assert!(transaction.actions.actions.is_empty());
        for tick in 2..=20 {
            frame.tick = tick;
            frame.now_millis += 50;
            assert!(air.step(&frame).packets.is_empty());
        }
        let repeated = inventory.predicted_selection(&server, 1).unwrap();
        let change = inventory.prepare_change(&repeated, air.next_legacy_request_id());
        assert_eq!(change.legacy_request_id, -8);
        assert_eq!(change.from.count(), 2);
        assert_eq!(change.to.count(), 1);
    }

    #[test]
    fn placing_the_last_block_has_no_air_fallback() {
        let server = selected(1);
        let mut inventory = HeldPlacementInventory::default();
        let mut air = crate::item_use::ItemUseRuntime::default();
        let change = inventory.prepare_change(&server, air.next_legacy_request_id());
        inventory.commit(server.clone(), 1, change);
        air.observe_press(true);
        let outcome = air.step(&crate::item_use::UseFrame {
            tick: 1,
            now_millis: 1_000,
            position: [0.5, 65.62, 0.5],
            held: true,
            selection: inventory.predicted_selection(&server, 1),
            air_use: None,
            ready: false,
            creative: false,
            inventory_revision: Some(1),
            charge_projectile: None,
            press_consumed: false,
        });
        assert!(outcome.packets.is_empty());
        assert_eq!(air.next_legacy_request_id(), -6);
    }
}
