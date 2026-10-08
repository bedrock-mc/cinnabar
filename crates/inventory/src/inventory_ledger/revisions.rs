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

    /// Empties a slot whose last item a throw consumed, unless the server wrote it after `revision`.
    /// Vanilla stamps no legacy request on an empty result, so the server answers only to correct it.
    pub fn settle_use_emptied_slot(&mut self, slot: u8, revision: u64) -> bool {
        let cell = Cell::Inventory(slot);
        if self.authoritative_slot_revision(slot) != Some(revision)
            || self.queue.iter().any(|request| request.touches(cell))
        {
            return false;
        }
        self.confirmed.set(cell, None);
        self.refold();
        true
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
    use crate::PlayerInventorySlot;

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

    fn pearl_slot(slot: u16, count: u16) -> InventoryEvent {
        InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot,
            },
            stack: NetworkItemStack {
                network_id: 422,
                stack_network_id: 41,
                count,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        })
    }

    // Servers stay silent when they agree with a throw's empty result, so the last pearl must not linger.
    #[test]
    fn throwing_the_last_item_presents_the_slot_empty_without_a_server_write() {
        let mut session = crate::InventorySession::new(1);
        session.ledger_mut().apply(&pearl_slot(2, 1));
        session.set_local_selected_slot(2);
        let revision = session.ledger().authoritative_slot_revision(2).unwrap();

        assert!(session.ledger_mut().settle_use_emptied_slot(2, revision));
        assert_eq!(
            session.ledger().slot_state(2),
            Some(PlayerInventorySlot::Empty)
        );
        assert_eq!(session.presented_hotbar_stack(2, None), None);
        assert_eq!(session.selected_stack(None), None);

        // A server that does confirm the empty slot changes nothing.
        session
            .ledger_mut()
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: 2,
                },
                stack: NetworkItemStack::empty(),
                storage_item: None,
            }));
        assert_eq!(session.presented_hotbar_stack(2, None), None);
    }

    // A rejected throw comes back as a restatement, and a write after the throw is never overwritten.
    #[test]
    fn server_writes_win_over_a_use_emptied_slot() {
        let mut ledger = PlayerInventoryLedger::default();
        ledger.apply(&pearl_slot(2, 1));
        assert!(ledger.settle_use_emptied_slot(2, 1));
        ledger.apply(&pearl_slot(2, 1));
        assert_eq!(ledger.displayed_stack(2).map(|stack| stack.count), Some(1));

        assert!(!ledger.settle_use_emptied_slot(2, 1));
        assert_eq!(ledger.displayed_stack(2).map(|stack| stack.count), Some(1));
        assert!(!ledger.settle_use_emptied_slot(5, 0));
        assert_eq!(ledger.slot_state(5), Some(PlayerInventorySlot::Unknown));
    }
}
