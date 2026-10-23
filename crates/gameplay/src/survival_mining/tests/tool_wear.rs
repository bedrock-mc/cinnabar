//! Client-authoritative breaks report a worn tool through their transaction.

use super::*;

/// Starts with one damaged sword in a known hotbar slot.
fn worn_sword() -> MiningAdmission {
    let mut harness = MiningAdmission::new(ticker_with_ticks(0));
    harness
        .inventory
        .apply(&protocol::InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
    let extra_data = protocol::edit_item_extra(
        &NetworkItemStack::empty().extra_data,
        &protocol::ItemNbtEdit {
            damage: Some(4),
            ..Default::default()
        },
    )
    .unwrap();
    use sha2::Digest;
    harness.inventory.apply(&protocol::InventoryEvent::Slot(
        protocol::InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: protocol::ContainerIdentity::window(0),
                slot: 2,
            },
            stack: NetworkItemStack {
                network_id: 300,
                count: 1,
                stack_network_id: 12,
                nbt_digest: sha2::Sha256::digest(&extra_data).into(),
                extra_data,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        },
    ));
    harness
}

/// Completes one break using the same current inventory facts as the edge producer.
fn break_with_sword(harness: &mut MiningAdmission, position: [i32; 3]) {
    let stack = harness.inventory.displayed_stack(2).unwrap().clone();
    let damage = harness
        .inventory
        .predicted_slot_damage(2)
        .or_else(|| protocol::item_extra_damage(&stack.extra_data).map(|damage| damage as i32))
        .unwrap();
    let target = DestroyTarget {
        selection: FrozenMiningSelection {
            slot: 2,
            item: VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap(),
        },
        wear: Some(ToolWear {
            current_damage: damage,
            break_damage: 2,
            maximum: Some(250),
        }),
        ..target(position, "minecraft:dirt", Some("minecraft:iron_sword"))
    };
    let before = harness.predictions.len();
    for _ in 0..100 {
        let tick = harness.ticker.completed_tick() + 1;
        harness
            .ticker
            .enqueue_completed_physics(completed(tick))
            .unwrap();
        harness.frame(DestroyInput::Held(Some(&target)), Client);
        let mut receipts = Vec::new();
        flush_player_auth_inputs(
            &mut harness.ticker,
            1,
            Some(evidence()),
            |identity, packet| {
                receipts.push(identity);
                harness.packets.push(packet);
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        for receipt in receipts {
            assert!(harness.ticker.acknowledge_physics_send(receipt));
        }
        let _ = harness.ticker.take_tick_evidence();
        if harness.predictions.len() != before {
            return;
        }
    }
    panic!("the sword did not complete a break");
}

#[test]
fn consecutive_client_breaks_chain_the_exact_worn_slot_descriptors() {
    use protocol::test_support::transactions::{
        InventorySourceKind, ItemUseAction, observe_input_transaction,
    };
    let mut harness = worn_sword();
    break_with_sword(&mut harness, [1, 2, 3]);
    break_with_sword(&mut harness, [2, 2, 3]);
    let transactions: Vec<_> = harness
        .packets
        .iter()
        .filter_map(observe_input_transaction)
        .collect();
    assert_eq!(transactions.len(), 2);
    let first = &transactions[0];
    let second = &transactions[1];
    for (transaction, damage, position) in [(first, 6, [1, 2, 3]), (second, 8, [2, 2, 3])] {
        assert!(protocol::is_legacy_request_id(
            transaction.legacy_request_id
        ));
        assert_eq!(
            transaction.legacy_set_slots,
            Some(vec![(protocol::CONTAINER_NAME_INVENTORY, vec![2])])
        );
        assert_eq!(transaction.actions.len(), 1);
        let action = &transaction.actions[0];
        assert_eq!(
            (action.source, action.container_id, action.slot),
            (InventorySourceKind::Container, Some(0), 2)
        );
        assert_eq!(action.from_item.stack_network_id, None);
        assert_eq!(action.to_item.stack_network_id, None);
        assert_eq!(
            protocol::item_extra_damage(&action.to_item.user_data),
            Some(damage)
        );
        let body = transaction.item_use().unwrap();
        assert_eq!(body.action, ItemUseAction::BreakBlock);
        assert_eq!((body.slot, body.position, body.face), (2, position, 1));
        assert_eq!(body.item, action.from_item);
    }
    assert_ne!(first.legacy_request_id, second.legacy_request_id);
    assert_eq!(
        harness
            .inventory
            .displayed_stack(2)
            .unwrap()
            .stack_network_id,
        second.legacy_request_id
    );
    assert_eq!(second.actions[0].from_item, first.actions[0].to_item);
    assert_eq!(
        protocol::item_extra_damage(&harness.inventory.displayed_stack(2).unwrap().extra_data),
        Some(8)
    );
}

/// Replies to one recorded break using a server-owned stack name and durability.
fn correct_break(harness: &mut MiningAdmission, request_id: i32, accepted: bool, damage: i32) {
    harness.inventory.apply(&protocol::InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: [protocol::StackResponse {
                status: if accepted {
                    protocol::StackResponseStatus::Accepted
                } else {
                    protocol::StackResponseStatus::Rejected
                },
                request_id,
                containers: [protocol::StackResponseContainer {
                    container: protocol::ContainerIdentity {
                        window_id: None,
                        slot_type: Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                        dynamic_id: None,
                    },
                    slots: [protocol::StackResponseSlot {
                        slot: 2,
                        hotbar_slot: 2,
                        count: 1,
                        item_stack_id: 77,
                        custom_name: Default::default(),
                        filtered_custom_name: Default::default(),
                        durability_correction: damage,
                    }]
                    .into(),
                }]
                .into(),
            }]
            .into(),
        },
    ));
}

#[test]
fn client_break_responses_correct_damage_and_reject_only_the_latest_change() {
    use protocol::test_support::transactions::observe_input_transaction;
    let mut harness = worn_sword();
    break_with_sword(&mut harness, [1, 2, 3]);
    let first = harness
        .packets
        .iter()
        .find_map(observe_input_transaction)
        .unwrap()
        .legacy_request_id;
    correct_break(&mut harness, first, true, 7);
    assert_eq!(
        harness
            .inventory
            .displayed_stack(2)
            .unwrap()
            .stack_network_id,
        77
    );
    break_with_sword(&mut harness, [2, 2, 3]);
    let second = harness
        .packets
        .iter()
        .filter_map(observe_input_transaction)
        .next_back()
        .unwrap();
    assert_eq!(second.actions[0].from_item.stack_network_id, None);
    assert_eq!(
        protocol::item_extra_damage(&second.actions[0].from_item.user_data),
        Some(7)
    );
    assert_eq!(
        protocol::item_extra_damage(&second.actions[0].to_item.user_data),
        Some(9)
    );
    correct_break(&mut harness, second.legacy_request_id, false, 0);
    let stack = harness.inventory.displayed_stack(2).unwrap().clone();
    assert_eq!(stack.stack_network_id, 77);
    assert_eq!(protocol::item_extra_damage(&stack.extra_data), Some(7));
    correct_break(&mut harness, second.legacy_request_id, true, 20);
    assert_eq!(harness.inventory.displayed_stack(2), Some(&stack));
}

#[test]
fn older_break_responses_preserve_later_wear_and_rebase_its_rollback() {
    use protocol::test_support::transactions::observe_input_transaction;
    let mut harness = worn_sword();
    break_with_sword(&mut harness, [1, 2, 3]);
    break_with_sword(&mut harness, [2, 2, 3]);
    let requests: Vec<_> = harness
        .packets
        .iter()
        .filter_map(observe_input_transaction)
        .map(|tx| tx.legacy_request_id)
        .collect();
    let latest = harness.inventory.displayed_stack(2).unwrap().clone();
    correct_break(&mut harness, requests[0], true, 7);
    assert_eq!(harness.inventory.displayed_stack(2), Some(&latest));
    assert_eq!(harness.inventory.predicted_slot_damage(2), Some(8));
    correct_break(&mut harness, requests[1], false, 0);
    let restored = harness.inventory.displayed_stack(2).unwrap();
    assert_eq!(restored.stack_network_id, 77);
    assert_eq!(protocol::item_extra_damage(&restored.extra_data), Some(7));
}

#[test]
fn wear_commits_only_on_attachment_and_transport_retries_keep_the_frozen_change() {
    let mut harness = worn_sword();
    let stack = harness.inventory.displayed_stack(2).unwrap().clone();
    let target = DestroyTarget {
        selection: FrozenMiningSelection {
            slot: 2,
            item: VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap(),
        },
        wear: Some(ToolWear {
            current_damage: 4,
            break_damage: 2,
            maximum: Some(250),
        }),
        ..target([1, 2, 3], "minecraft:dirt", Some("minecraft:iron_sword"))
    };
    let mut machine = DestroyMachine::default();
    let payload = (0..100)
        .map(|_| held(&mut machine, &target, Client))
        .find(|payload| payload.destroy.is_some())
        .unwrap();
    let mut refused = crate::survival_mining::MiningAdmission::default();
    assert!(
        !payload
            .clone()
            .attach(&mut harness.ticker, 101, &mut refused)
    );
    refused.settle_inventory(&mut harness.inventory);
    assert_eq!(harness.inventory.displayed_stack(2), Some(&stack));

    harness
        .ticker
        .enqueue_completed_physics(completed(101))
        .unwrap();
    let mut admitted = crate::survival_mining::MiningAdmission::default();
    assert!(payload.attach(&mut harness.ticker, 101, &mut admitted));
    admitted.settle_inventory(&mut harness.inventory);
    let mut attempted = None;
    assert!(
        flush_player_auth_inputs(&mut harness.ticker, 1, Some(evidence()), |_, packet| {
            attempted = Some(packet);
            Err("full")
        })
        .is_err()
    );
    let attempted = attempted.unwrap();
    let transaction =
        protocol::test_support::transactions::observe_input_transaction(&attempted).unwrap();
    correct_break(&mut harness, transaction.legacy_request_id, true, 7);
    let corrected = harness.inventory.displayed_stack(2).unwrap().clone();
    flush_player_auth_inputs(&mut harness.ticker, 1, Some(evidence()), |_, packet| {
        assert_eq!(
            protocol::encode(&packet).unwrap(),
            protocol::encode(&attempted).unwrap()
        );
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(harness.inventory.displayed_stack(2), Some(&corrected));
}

/// A client-authoritative break that wears the held tool reports the worn stack as the
/// transaction's slot change under a fresh legacy request; an unworn or breaking tool follows
/// the same rule as the stack it leaves.
#[test]
fn client_destroys_carry_the_worn_tool_as_their_slot_change() {
    let sword = VerifiedNetworkItemStack::try_new(
        NetworkItemStack {
            network_id: 300,
            count: 1,
            stack_network_id: 12,
            ..NetworkItemStack::empty()
        },
        NetworkItemStack::empty().nbt_digest,
    )
    .unwrap();
    let complete = |current_damage, break_damage, maximum| {
        let mut target = DestroyTarget {
            wear: Some(ToolWear {
                current_damage,
                break_damage,
                maximum,
            }),
            ..target([0, 0, 0], "minecraft:dirt", Some("minecraft:iron_sword"))
        };
        target.selection.item = sword.clone();
        let mut machine = DestroyMachine::default();
        held(&mut machine, &target, Client);
        let done = loop {
            let payload = held(&mut machine, &target, Client);
            if payload.destroy.is_some() {
                break payload;
            }
        };
        let (interactions, request) = done.into_interactions([0.5, 64.0, 0.5]);
        assert_eq!(request, None, "client authority sends no mining request");
        match interactions.block_interaction {
            Some(protocol::BlockItemInteraction::Destroy(_, change)) => change,
            other => panic!("expected a destroy transaction, got {other:?}"),
        }
    };
    let change = complete(4, 2, Some(250)).expect("a worn tool");
    assert_eq!(change.from, sword);
    assert!(protocol::is_legacy_request_id(change.legacy_request_id));
    assert_eq!(change.to.stack_network_id(), change.legacy_request_id);
    assert_eq!(change.to.network_id(), 300);
    assert_eq!(change.to.count(), 1);
    assert_eq!(protocol::item_extra_damage(change.to.extra_data()), Some(6));
    assert_eq!(
        complete(4, 0, Some(250)),
        None,
        "a spared tool is unchanged"
    );
    let broken = complete(249, 2, Some(250)).expect("a breaking tool");
    assert!(
        broken.to.is_empty(),
        "damage past the maximum breaks the tool"
    );
}
