//! Per-slot authoritative write epochs for locally predicted item-use state.

use super::{Cell, PlayerInventoryLedger};

impl PlayerInventoryLedger {
    /// A server restatement advances this epoch even when its bytes are unchanged,
    /// so rejected item-use predictions cannot survive an identical correction.
    pub fn authoritative_slot_revision(&self, slot: u8) -> Option<u64> {
        self.known
            .get(usize::from(slot))?
            .then_some(self.slot_revisions[usize::from(slot)])
    }

    pub(super) fn note_authoritative_write(&mut self, cell: Cell) {
        if let Cell::Inventory(slot) = cell
            && let Some(revision) = self.slot_revisions.get_mut(usize::from(slot))
        {
            *revision = revision.wrapping_add(1).max(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use protocol::{
        ContainerIdentity, InventoryContentEvent, InventoryEvent, InventorySlotEvent,
        NetworkItemStack, SlotIdentity,
    };

    use super::*;

    #[test]
    fn identical_slot_restatements_advance_only_the_addressed_slot() {
        let mut ledger = PlayerInventoryLedger::default();
        assert_eq!(ledger.authoritative_slot_revision(0), None);
        let event = InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 0,
            },
            stack: NetworkItemStack::empty(),
            storage_item: None,
        });
        ledger.apply(&event);
        assert_eq!(ledger.authoritative_slot_revision(0), Some(1));
        ledger.apply(&event);
        assert_eq!(ledger.authoritative_slot_revision(0), Some(2));
        assert_eq!(ledger.authoritative_slot_revision(1), None);
        ledger.begin_session(2);
        assert_eq!(ledger.authoritative_slot_revision(0), None);
    }

    #[test]
    fn content_restatements_advance_each_retained_player_slot_not_other_surfaces() {
        let mut ledger = PlayerInventoryLedger::default();
        let content = |window| {
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(window),
                slots: vec![NetworkItemStack::empty(); 2].into(),
                storage_item: NetworkItemStack::empty(),
            })
        };
        ledger.apply(&content(0));
        ledger.apply(&content(0));
        assert_eq!(ledger.authoritative_slot_revision(0), Some(2));
        assert_eq!(ledger.authoritative_slot_revision(1), Some(2));
        assert_eq!(ledger.authoritative_slot_revision(2), None);
        ledger.apply(&content(119));
        assert_eq!(ledger.authoritative_slot_revision(0), Some(2));
    }

    #[test]
    fn identical_transaction_stack_restatements_advance_only_the_addressed_slot() {
        let mut ledger = PlayerInventoryLedger::default();
        let event = InventoryEvent::Transaction(protocol::InventoryTransactionEvent {
            slots: vec![InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                    slot: 2,
                },
                stack: NetworkItemStack::empty(),
                storage_item: None,
            }]
            .into(),
            skipped_actions: 0,
        });
        ledger.apply(&event);
        ledger.apply(&event);
        assert_eq!(ledger.authoritative_slot_revision(2), Some(2));
        assert_eq!(ledger.authoritative_slot_revision(1), None);
        assert_eq!(ledger.authoritative_slot_revision(3), None);
    }
}
