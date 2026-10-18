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

/// Admits one decrement from the current backing item and returns its transaction identity.
fn consume_backing(ledger: &mut PlayerInventoryLedger, slot: u8) -> i32 {
    let original = ledger
        .confirmed_stack(Cell::Inventory(slot))
        .unwrap()
        .clone();
    let revision = ledger.authoritative_slot_revision(slot).unwrap();
    let from =
        protocol::VerifiedNetworkItemStack::try_new(original.clone(), original.nbt_digest).unwrap();
    let legacy_request_id = protocol::next_legacy_request_id().unwrap();
    let change = protocol::PredictedSlotChange {
        to: from.less_one(legacy_request_id),
        from,
        legacy_request_id,
    };
    assert!(ledger.settle_use_slot_change(slot, revision, &change));
    legacy_request_id
}

/// Older screen responses account for every later admitted consumption of their retained item.
#[test]
fn admitted_consumption_survives_accepted_screen_prediction() {
    for consumed in 1..=3 {
        for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
            let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
            let request = ledger
                .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
                .unwrap();
            send_all(&mut ledger);
            let mut latest = 0;
            for _ in 0..consumed {
                latest = consume_backing(&mut ledger, 0);
            }
            respond(
                &mut ledger,
                request,
                status,
                if status == StackResponseStatus::Accepted {
                    vec![correction(
                        CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                        0,
                        3,
                        91,
                    )]
                } else {
                    vec![]
                },
            );
            let expected = if status == StackResponseStatus::Accepted {
                3 - consumed
            } else {
                4 - consumed
            };
            let backing = ledger.confirmed_stack(Cell::Inventory(0));
            assert_eq!(backing.map_or(0, |stack| stack.count), expected);
            if let Some(backing) = backing {
                assert_eq!(backing.stack_network_id, latest);
            }
            assert_eq!(ledger.displayed_stack(0), backing);
        }
    }
}

/// Consumption after an identical authoritative restatement still outlives the screen response.
#[test]
fn consumption_after_identical_restatement_survives_older_screen_response() {
    let original = stack(6, 10, 4);
    let mut ledger = open_ledger(&[(0, original.clone())]);
    let request = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    send_all(&mut ledger);
    ledger.apply(&slot_push(0, original));
    let latest = consume_backing(&mut ledger, 0);
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            3,
            91,
        )],
    );
    let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
    assert_eq!((backing.count, backing.stack_network_id), (2, latest));
    assert_eq!(ledger.displayed_stack(0), Some(backing));
}

/// Unsent requests are rebuilt before transport without a second consumption offset.
#[test]
fn admitted_consumption_is_not_counted_twice_for_an_unsent_screen_request() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let request = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    consume_backing(&mut ledger, 0);
    send_all(&mut ledger);
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            2,
            91,
        )],
    );
    let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
    assert_eq!((backing.count, backing.stack_network_id), (2, 91));
}

/// A replacement stack does not inherit consumption of the previous backing item.
#[test]
fn admitted_consumption_does_not_shrink_an_accepted_screen_swap() {
    for replacement_id in [6, 7] {
        let mut ledger = open_ledger(&[(0, stack(6, 10, 4)), (1, stack(replacement_id, 11, 1))]);
        let request = ledger
            .begin_hotbar_swap(InventoryTarget::Player(0), 1)
            .unwrap();
        send_all(&mut ledger);
        consume_backing(&mut ledger, 0);
        respond(
            &mut ledger,
            request,
            StackResponseStatus::Accepted,
            vec![correction(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                1,
                91,
            )],
        );
        let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
        assert_eq!(
            (backing.network_id, backing.count, backing.stack_network_id),
            (replacement_id, 1, 91)
        );
    }
}

/// HUD mining corrections retain later consumption just as front-screen corrections do.
#[test]
fn admitted_consumption_survives_an_older_mining_correction() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let request = ledger.begin_mining_request(0, 0).unwrap();
    let latest = consume_backing(&mut ledger, 0);
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            4,
            91,
        )],
    );
    let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
    assert_eq!((backing.count, backing.stack_network_id), (3, latest));
}

/// A later drop keeps the replacement stack's provenance through an earlier identical-item swap.
#[test]
fn admitted_consumption_does_not_shrink_a_drop_of_a_swapped_identical_stack() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4)), (1, stack(6, 11, 4))]);
    ledger
        .begin_hotbar_swap(InventoryTarget::Player(0), 1)
        .unwrap();
    let request = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    send_all(&mut ledger);
    consume_backing(&mut ledger, 0);
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            3,
            91,
        )],
    );
    let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
    assert_eq!((backing.count, backing.stack_network_id), (3, 91));
}

/// A fresh server write cancels earlier local consumption before an older response arrives.
#[test]
fn authoritative_restatement_retires_consumption_offsets() {
    for mining in [false, true] {
        let original = stack(6, 10, 4);
        let mut ledger = open_ledger(&[(0, original.clone())]);
        let request = if mining {
            ledger.begin_mining_request(0, 0).unwrap()
        } else {
            let request = ledger
                .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
                .unwrap();
            send_all(&mut ledger);
            request
        };
        consume_backing(&mut ledger, 0);
        ledger.apply(&slot_push(0, original));
        let count = if mining { 4 } else { 3 };
        respond(
            &mut ledger,
            request,
            StackResponseStatus::Accepted,
            vec![correction(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                count,
                91,
            )],
        );
        let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
        assert_eq!(
            (backing.count, backing.stack_network_id),
            (u16::from(count), 91)
        );
    }
}

#[path = "world_use/history.rs"]
mod history;

#[path = "world_use/cross_cell.rs"]
mod cross_cell;

#[path = "world_use/merges.rs"]
mod merges;
#[path = "world_use/queued_drops.rs"]
mod queued_drops;
