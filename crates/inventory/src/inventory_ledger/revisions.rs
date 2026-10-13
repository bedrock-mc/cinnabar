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
        self.note_observed_gear(cell);
        if let Cell::Inventory(slot) = cell
            && let Some(revision) = self.slot_revisions.get_mut(usize::from(slot))
        {
            *revision = revision.wrapping_add(1).max(1);
        }
    }

    pub(super) fn note_observed_gear(&mut self, cell: Cell) {
        match cell {
            Cell::Armor(slot) => {
                if let Some(known) = self.armor_known.get_mut(usize::from(slot)) {
                    *known = true;
                }
            }
            Cell::Offhand => self.offhand_known = true,
            _ => {}
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

    /// Builds one admitted decrement for a known backing stack.
    fn change(ledger: &PlayerInventoryLedger, slot: u8) -> protocol::PredictedSlotChange {
        let stack = ledger
            .confirmed_stack(Cell::Inventory(slot))
            .unwrap()
            .clone();
        let from =
            protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
        let legacy_request_id = protocol::next_legacy_request_id().unwrap();
        protocol::PredictedSlotChange {
            to: from.less_one(legacy_request_id),
            from,
            legacy_request_id,
        }
    }

    // Servers stay silent when they agree with a throw's empty result, so the last pearl must not linger.
    #[test]
    fn throwing_the_last_item_presents_the_slot_empty_without_a_server_write() {
        let mut session = crate::InventorySession::new(1);
        session.ledger_mut().apply(&pearl_slot(2, 1));
        session.set_local_selected_slot(2);
        let revision = session.ledger().authoritative_slot_revision(2).unwrap();

        let change = change(session.ledger(), 2);
        assert!(
            session
                .ledger_mut()
                .settle_use_slot_change(2, revision, &change)
        );
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
        let change = change(&ledger, 2);
        assert!(ledger.settle_use_slot_change(2, 1, &change));
        ledger.apply(&pearl_slot(2, 1));
        assert_eq!(ledger.displayed_stack(2).map(|stack| stack.count), Some(1));

        assert!(!ledger.settle_use_slot_change(2, 1, &change));
        assert_eq!(ledger.displayed_stack(2).map(|stack| stack.count), Some(1));
        assert!(!ledger.settle_use_slot_change(5, 0, &change));
        assert_eq!(ledger.slot_state(5), Some(PlayerInventorySlot::Unknown));
    }
}
