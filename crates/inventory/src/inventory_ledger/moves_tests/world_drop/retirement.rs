use super::*;

/// Admits a screen drop, its close, then a gameplay drop before the close acknowledgement.
fn closing_drops() -> (PlayerInventoryLedger, i32, i32, i32) {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    open_personal(&mut ledger);
    let screen = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
        .unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(21));
    let world = ledger.begin_world_drop(2, Some(1)).unwrap();
    let legacy = drop_legacy_id(&ledger, world);
    assert!(ledger.mark_transport_enqueued(22));
    (ledger, screen, world, legacy)
}

/// Completes the real admitted personal close without answering any item request.
fn close_ack(ledger: &mut PlayerInventoryLedger) {
    ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: protocol::NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
}

/// An unresolved retired predecessor leaves an explicitly unverified rollback, never its dead alias.
#[test]
fn close_ack_between_drop_answers_does_not_restore_a_deleted_owner() {
    let (mut ledger, screen, _, legacy) = closing_drops();
    close_ack(&mut ledger);
    ledger.apply(&response(screen, StackResponseStatus::Rejected, None));
    ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
    let restored = ledger.displayed_stack(2).unwrap().clone();
    let next = ledger.begin_world_drop(2, Some(1));
    eprintln!(
        "retirement rollback count={} id={} next={next:?}",
        restored.count, restored.stack_network_id
    );
    assert_eq!(
        (restored.count, restored.stack_network_id, next),
        (4, 0, Err(InventoryGestureError::ResyncRequired))
    );
    assert!(ledger.resync_required());
}

/// Explicitly accepted gameplay identity remains usable after its screen predecessor retires.
#[test]
fn accepted_world_successor_keeps_a_transportable_identity() {
    let (mut ledger, screen, _, legacy) = closing_drops();
    close_ack(&mut ledger);
    ledger.apply(&response(screen, StackResponseStatus::Rejected, None));
    ledger.apply(&response(
        legacy,
        StackResponseStatus::Accepted,
        Some((3, 93)),
    ));
    assert_eq!(
        ledger
            .displayed_stack(2)
            .map(|held| (held.count, held.stack_network_id)),
        Some((3, 93))
    );
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    let transaction = observe_transaction(&packet).unwrap();
    assert_eq!(transaction.actions[0].from_item.stack_network_id, Some(93));
    assert_eq!(transaction.actions[0].from_item.count, 3);
}

/// A predecessor answered before retirement supplies a known rollback in either outcome.
#[test]
fn answered_predecessors_keep_known_rollback_and_next_transport() {
    for accepted in [false, true] {
        let (mut ledger, screen, _, legacy) = closing_drops();
        ledger.apply(&response(
            screen,
            if accepted {
                StackResponseStatus::Accepted
            } else {
                StackResponseStatus::Rejected
            },
            accepted.then_some((4, 91)),
        ));
        close_ack(&mut ledger);
        ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
        let expected = if accepted { (4, 91) } else { (5, 100) };
        assert_eq!(
            ledger
                .displayed_stack(2)
                .map(|held| (held.count, held.stack_network_id)),
            Some(expected)
        );
        ledger.begin_world_drop(2, Some(1)).unwrap();
        let (packet, _) = ledger.pending_batch().unwrap().unwrap();
        let transaction = observe_transaction(&packet).unwrap();
        assert_eq!(
            (
                transaction.actions[0].from_item.count,
                transaction.actions[0].from_item.stack_network_id
            ),
            (expected.0, Some(expected.1))
        );
    }
}

/// Restates the full player surface, establishing usable authority after an unresolved rollback.
fn refresh_player(ledger: &mut PlayerInventoryLedger, held: NetworkItemStack) {
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[2] = held;
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
}

/// Recovery remains explicit until a real authoritative refresh establishes the next descriptor.
#[test]
fn unknown_retired_source_recovers_from_fresh_authority() {
    let (mut ledger, _, _, legacy) = closing_drops();
    close_ack(&mut ledger);
    ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
    assert_eq!(
        ledger.begin_world_drop(2, Some(1)),
        Err(InventoryGestureError::ResyncRequired)
    );
    let replacement = digested(stack(8, 202, 7));
    refresh_player(&mut ledger, replacement.clone());
    assert_eq!(ledger.displayed_stack(2), Some(&replacement));
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    let transaction = observe_transaction(&packet).unwrap();
    assert_eq!(
        (
            transaction.actions[0].from_item.network_id,
            transaction.actions[0].from_item.count,
            transaction.actions[0].from_item.stack_network_id
        ),
        (8, 7, Some(202))
    );
}

/// A replacement arriving before rejection invalidates history rather than inheriting old data.
#[test]
fn fresh_replacement_invalidates_retired_dependent_history() {
    let (mut ledger, _, _, legacy) = closing_drops();
    close_ack(&mut ledger);
    let replacement = digested(stack(8, 202, 7));
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 2,
        },
        stack: replacement.clone(),
        storage_item: None,
    }));
    ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
    assert_eq!(ledger.displayed_stack(2), Some(&replacement));
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    let transaction = observe_transaction(&packet).unwrap();
    assert_eq!(transaction.actions[0].from_item.stack_network_id, Some(202));
}

/// Multiple admitted descendants propagate unresolved authority without disturbing another cell.
#[test]
fn dependent_world_history_survives_retirement_in_both_reply_orders() {
    for newest_first in [false, true] {
        let (mut ledger, _, _, first) = closing_drops();
        let other = digested(stack(8, 200, 5));
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 3,
            },
            stack: other.clone(),
            storage_item: None,
        }));
        let unrelated = ledger.begin_world_drop(3, Some(1)).unwrap();
        let unrelated_legacy = drop_legacy_id(&ledger, unrelated);
        assert!(ledger.mark_transport_enqueued(23));
        let second = ledger.begin_world_drop(2, Some(1)).unwrap();
        let second_legacy = drop_legacy_id(&ledger, second);
        assert!(ledger.mark_transport_enqueued(24));
        close_ack(&mut ledger);
        ledger.apply(&response(
            unrelated_legacy,
            StackResponseStatus::Rejected,
            None,
        ));
        assert_eq!(ledger.displayed_stack(3), Some(&other));
        for id in if newest_first {
            [second_legacy, first]
        } else {
            [first, second_legacy]
        } {
            ledger.apply(&response(id, StackResponseStatus::Rejected, None));
        }
        assert_eq!(
            ledger
                .displayed_stack(2)
                .map(|held| (held.count, held.stack_network_id)),
            Some((4, 0))
        );
        assert_eq!(ledger.displayed_stack(3), Some(&other));
        assert_eq!(
            ledger.begin_world_drop(2, Some(1)),
            Err(InventoryGestureError::ResyncRequired)
        );
    }
}

/// The latest accepted descendant supplies authority despite an unresolved retired ancestor.
#[test]
fn accepted_descendant_supersedes_unverified_ancestor_history() {
    for newest_first in [false, true] {
        let (mut ledger, _, _, first) = closing_drops();
        let second = ledger.begin_world_drop(2, Some(1)).unwrap();
        let second_legacy = drop_legacy_id(&ledger, second);
        assert!(ledger.mark_transport_enqueued(23));
        close_ack(&mut ledger);
        let rejected = response(first, StackResponseStatus::Rejected, None);
        let accepted = response(second_legacy, StackResponseStatus::Accepted, Some((2, 94)));
        for answer in if newest_first {
            [accepted, rejected]
        } else {
            [rejected, accepted]
        } {
            ledger.apply(&answer);
        }
        assert_eq!(
            ledger
                .displayed_stack(2)
                .map(|held| (held.count, held.stack_network_id)),
            Some((2, 94))
        );
        assert!(!ledger.resync_required());
        ledger.begin_world_drop(2, Some(1)).unwrap();
        let (packet, _) = ledger.pending_batch().unwrap().unwrap();
        assert_eq!(
            observe_transaction(&packet).unwrap().actions[0]
                .from_item
                .stack_network_id,
            Some(94)
        );
    }
}

/// Admitted gameplay work survives each storage-screen retirement path independently.
#[test]
fn storage_retirement_preserves_admitted_world_successors() {
    for path in ["ack", "replace", "server_close"] {
        let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
        ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(3),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
        let screen = ledger
            .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
            .unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        let world = ledger.begin_world_drop(2, Some(1)).unwrap();
        let legacy = drop_legacy_id(&ledger, world);
        assert!(ledger.mark_transport_enqueued(21));
        match path {
            "ack" => {
                ledger.request_storage_close();
                assert!(ledger.mark_transport_enqueued(22));
                ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
                    container: ContainerIdentity::window(3),
                    window_type: protocol::NO_CONTAINER_WINDOW_TYPE,
                    server_initiated: false,
                }));
            }
            "replace" => ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(4),
                window_type: GENERIC_STORAGE_WINDOW_TYPE,
                position: [1, 64, 0],
                runtime_entity_id: -1,
            })),
            "server_close" => ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
                container: ContainerIdentity::window(3),
                window_type: GENERIC_STORAGE_WINDOW_TYPE,
                server_initiated: true,
            })),
            _ => unreachable!(),
        }
        ledger.apply(&response(screen, StackResponseStatus::Rejected, None));
        ledger.apply(&response(
            legacy,
            StackResponseStatus::Accepted,
            Some((3, 95)),
        ));
        assert_eq!(
            ledger
                .displayed_stack(2)
                .map(|held| (held.count, held.stack_network_id)),
            Some((3, 95)),
            "{path}"
        );
    }
}

/// Retirement also tracks a pending empty source instead of assuming it was authoritative empty.
#[test]
fn empty_predecessor_rollback_stays_unverified_after_retirement() {
    let mut ledger = known_ledger(&[
        (2, digested(stack(6, 100, 5))),
        (20, digested(stack(8, 200, 3))),
    ]);
    open_personal(&mut ledger);
    ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), None)
        .unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    let swap = ledger
        .begin_windowless_hotbar_swap(InventoryTarget::Player(20), 2)
        .unwrap();
    assert!(ledger.mark_transport_enqueued(21));
    let world = ledger.begin_world_drop(2, Some(1)).unwrap();
    let legacy = drop_legacy_id(&ledger, world);
    assert!(ledger.mark_transport_enqueued(22));
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(23));
    close_ack(&mut ledger);
    ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
    ledger.apply(&response(swap, StackResponseStatus::Rejected, None));
    assert!(ledger.displayed_stack(2).is_none());
    assert_eq!(
        ledger
            .displayed_stack(20)
            .map(|held| (held.count, held.stack_network_id)),
        Some((3, 200))
    );
    assert_eq!(
        ledger.begin_world_drop(20, Some(1)),
        Err(InventoryGestureError::ResyncRequired)
    );
}

/// Losing the last alias owner through a reset or teardown marks retained backing unverified.
#[test]
fn reset_and_teardown_never_leave_a_retired_alias_in_backing() {
    for transport in [false, true] {
        let (mut ledger, screen, _, legacy) = closing_drops();
        ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
        assert_eq!(ledger.displayed_stack(2).unwrap().stack_network_id, screen);
        if transport {
            ledger.transport_closed();
        } else {
            ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
        }
        assert_eq!(
            ledger
                .displayed_stack(2)
                .map(|held| (held.count, held.stack_network_id)),
            Some((4, 0))
        );
        assert_eq!(
            ledger.begin_world_drop(2, Some(1)),
            Err(InventoryGestureError::ResyncRequired)
        );
    }
}

/// A dependent drop still waiting for transport is cancelled when its source loses authority.
#[test]
fn retirement_cancels_unsent_dependants_before_building_a_transaction() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    open_personal(&mut ledger);
    ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
        .unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(21));
    ledger.begin_world_drop(2, Some(1)).unwrap();
    close_ack(&mut ledger);
    assert!(ledger.pending_batch().unwrap().is_none());
    assert_eq!(
        ledger
            .displayed_stack(2)
            .map(|held| (held.count, held.stack_network_id)),
        Some((5, 100))
    );
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(
        observe_transaction(&packet).unwrap().actions[0]
            .from_item
            .stack_network_id,
        Some(100)
    );
}

/// An old empty-cell owner cannot claim an independently admitted nonempty gameplay remainder.
#[test]
fn retiring_an_empty_owner_keeps_a_known_nonempty_local_source() {
    let mut ledger = known_ledger(&[
        (2, digested(stack(6, 100, 5))),
        (20, digested(stack(8, 200, 3))),
    ]);
    open_personal(&mut ledger);
    ledger.begin_click(2).unwrap();
    let place = ledger.begin_click(2).unwrap();
    let first = ledger.begin_world_drop(2, None).unwrap();
    let first_legacy = drop_legacy_id(&ledger, first);
    while ledger.pending_batch().unwrap().is_some() {
        assert!(ledger.mark_transport_enqueued(20));
    }
    ledger.apply(&response(place, StackResponseStatus::Rejected, None));
    ledger.apply(&response(first_legacy, StackResponseStatus::Rejected, None));
    assert!(ledger.displayed_stack(2).is_none());
    ledger
        .begin_windowless_hotbar_swap(InventoryTarget::Player(20), 2)
        .unwrap();
    assert!(ledger.mark_transport_enqueued(21));
    let second = ledger.begin_world_drop(2, Some(1)).unwrap();
    let second_legacy = drop_legacy_id(&ledger, second);
    assert!(ledger.mark_transport_enqueued(22));
    ledger.apply(&response(
        second_legacy,
        StackResponseStatus::Accepted,
        None,
    ));
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 2);
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(23));
    ledger.begin_world_drop(2, Some(1)).unwrap();
    close_ack(&mut ledger);
    let (packet, _) = ledger
        .pending_batch()
        .unwrap()
        .expect("independent gameplay drop remains queued");
    let transaction = observe_transaction(&packet).unwrap();
    assert_eq!(
        (
            transaction.actions[0].from_item.count,
            transaction.actions[0].from_item.stack_network_id
        ),
        (2, Some(second_legacy))
    );
}

/// Unnamed client-authority backing stays usable when unsent local work is cancelled.
#[test]
fn cancelling_local_work_preserves_an_unused_item_id() {
    let original = digested(stack(6, -1, 5));
    let mut ledger = known_ledger(&[(2, original.clone())]);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    open_personal(&mut ledger);
    ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
        .unwrap();
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(20));
    close_ack(&mut ledger);
    assert_eq!(ledger.displayed_stack(2), Some(&original));
    assert!(!ledger.resync_required());
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    let transaction = observe_transaction(&packet).unwrap();
    assert_eq!(transaction.actions[0].from_item.count, original.count);
}

/// Changing to server authority cannot turn an unassigned local identity into trusted backing.
#[test]
fn server_authority_transition_requires_assigned_backing_identities() {
    let original = digested(stack(6, 0, 5));
    let mut ledger = known_ledger(&[(2, original.clone())]);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    assert!(!ledger.resync_required());
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    assert_eq!(ledger.displayed_stack(2), Some(&original));
    assert_eq!(
        ledger.begin_world_drop(2, Some(1)),
        Err(InventoryGestureError::ResyncRequired)
    );
    let assigned = digested(stack(6, 101, 5));
    refresh_player(&mut ledger, assigned.clone());
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(
        observe_transaction(&packet).unwrap().actions[0]
            .from_item
            .stack_network_id,
        Some(assigned.stack_network_id)
    );
}

/// A HUD request with no stamped cell cannot authorize an equal server-provided wire ID.
#[test]
fn mining_request_counters_do_not_vouch_for_sparse_wire_ownership() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    let mining = ledger.begin_mining_request(2, 1).unwrap();
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 2,
        },
        stack: digested(stack(6, mining, 5)),
        storage_item: None,
    }));
    assert_eq!(
        ledger.begin_world_drop(2, Some(1)),
        Err(InventoryGestureError::AwaitingIdentity)
    );
    ledger.cancel_mining_request(mining);
    assert_eq!(
        ledger.begin_world_drop(2, Some(1)),
        Err(InventoryGestureError::AwaitingIdentity)
    );
    refresh_player(&mut ledger, digested(stack(6, 101, 5)));
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(
        observe_transaction(&packet).unwrap().actions[0]
            .from_item
            .stack_network_id,
        Some(101)
    );
}

/// Negotiation alone does not resolve the outcome of an already unverified rollback.
#[test]
fn explicit_rollback_recovery_survives_authority_changes() {
    let (mut ledger, _, _, legacy) = closing_drops();
    close_ack(&mut ledger);
    ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
    for authority in [InventoryAuthority::Client, InventoryAuthority::Server] {
        ledger.apply(&InventoryEvent::Authority(authority));
        assert_eq!(
            ledger
                .displayed_stack(2)
                .map(|held| (held.count, held.stack_network_id)),
            Some((4, 0))
        );
        assert_eq!(
            ledger.begin_world_drop(2, Some(1)),
            Err(InventoryGestureError::ResyncRequired)
        );
    }
    refresh_player(&mut ledger, digested(stack(6, 101, 5)));
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(
        observe_transaction(&packet).unwrap().actions[0]
            .from_item
            .stack_network_id,
        Some(101)
    );
}

/// An accepted transport alias can feed queued work without becoming settled rollback backing.
#[test]
fn accepted_alias_keeps_transport_but_requires_recovery_on_rollback() {
    let (mut ledger, screen, _, legacy) = closing_drops();
    ledger.apply(&response(screen, StackResponseStatus::Accepted, None));
    close_ack(&mut ledger);
    ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
    assert_eq!(
        ledger
            .displayed_stack(2)
            .map(|held| (held.count, held.stack_network_id)),
        Some((4, 0))
    );
    assert_eq!(
        ledger.begin_world_drop(2, Some(1)),
        Err(InventoryGestureError::ResyncRequired)
    );
}
