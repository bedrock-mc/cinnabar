use super::*;

/// A successful world decrement remains backing truth after a screen gesture is rejected.
#[test]
fn admitted_consumption_survives_rejected_screen_prediction() {
    for count in [1, 3] {
        for take in [false, true] {
            let original = stack(6, 10, count);
            let mut ledger = open_ledger(&[(0, original.clone())]);
            let screen = if take {
                ledger.begin_click(0).unwrap()
            } else {
                ledger
                    .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
                    .unwrap()
            };
            send_all(&mut ledger);
            let revision = ledger.authoritative_slot_revision(0).unwrap();
            let from =
                protocol::VerifiedNetworkItemStack::try_new(original.clone(), original.nbt_digest)
                    .unwrap();
            let legacy_request_id = protocol::next_legacy_request_id().unwrap();
            let change = protocol::PredictedSlotChange {
                to: from.less_one(legacy_request_id),
                from,
                legacy_request_id,
            };
            assert!(ledger.settle_use_slot_change(0, revision, &change));
            respond(&mut ledger, screen, StackResponseStatus::Rejected, vec![]);
            let backing = ledger.confirmed_stack(Cell::Inventory(0));
            assert_eq!(backing.map_or(0, |stack| stack.count), count - 1);
            if count > 1 {
                assert_eq!(backing.unwrap().stack_network_id, legacy_request_id);
            }
            assert_eq!(ledger.displayed_stack(0), backing);
        }
    }
}

/// Mining owns the backing tool even while a front prediction removes or replaces it.
#[test]
fn mining_correction_survives_rejected_front_replacement() {
    for replace in [false, true] {
        for mining_first in [false, true] {
            let mut ledger = open_ledger(&[(0, stack(6, 10, 1)), (1, stack(7, 11, 1))]);
            let mut screen = vec![ledger.begin_click(u8::from(replace)).unwrap()];
            if replace {
                screen.push(ledger.begin_click(0).unwrap());
            }
            send_all(&mut ledger);
            let mining = ledger.begin_mining_request(0, 9).unwrap();
            let mut corrected = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 1, 91);
            corrected.slots = Arc::from([StackResponseSlot {
                durability_correction: 9,
                ..corrected.slots[0].clone()
            }]);
            if mining_first {
                respond(
                    &mut ledger,
                    mining,
                    StackResponseStatus::Accepted,
                    vec![corrected.clone()],
                );
            }
            for request in screen.into_iter().rev() {
                respond(&mut ledger, request, StackResponseStatus::Rejected, vec![]);
            }
            if !mining_first {
                respond(
                    &mut ledger,
                    mining,
                    StackResponseStatus::Accepted,
                    vec![corrected],
                );
            }
            let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
            assert_eq!((backing.network_id, backing.stack_network_id), (6, 91));
            assert_eq!(
                protocol::item_extra_damage_value(&backing.extra_data),
                Some(9)
            );
        }
    }
}
