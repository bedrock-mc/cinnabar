use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CURSOR,
    CONTAINER_NAME_LEVEL_ENTITY, ContainerIdentity, ContainerOpenEvent, InventoryContentEvent,
    InventoryEvent, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion,
    ItemStackResponseEvent, NetworkItemStack, StackRequestAction, StackResponse,
    StackResponseContainer, StackResponseSlot, StackResponseStatus,
};
use sha2::{Digest, Sha256};

use super::*;

fn entry(
    network_id: i32,
    identifier: &str,
    negotiated: Option<u8>,
    component_based: bool,
    canonical_empty: bool,
) -> ItemRegistryEntry {
    ItemRegistryEntry {
        identifier: Arc::from(identifier),
        network_id,
        component_based,
        version: ItemRegistryVersion::DataDriven,
        component_digest: [network_id as u8; 32],
        negotiated_max_stack_size: negotiated,
        canonical_empty_component_data: canonical_empty,
    }
}

fn registry(entries: Vec<ItemRegistryEntry>) -> ItemRegistryEvent {
    ItemRegistryEvent {
        entries: entries.into(),
    }
}

fn stack(network_id: i32, stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        stack_network_id,
        count,
        ..NetworkItemStack::default()
    }
}

fn ten_zero_stack(network_id: i32, stack_network_id: i32, count: u16) -> NetworkItemStack {
    let extra_data: Arc<[u8]> = Arc::from([0; 10]);
    NetworkItemStack {
        network_id,
        stack_network_id,
        count,
        nbt_digest: Sha256::digest(&extra_data).into(),
        extra_data,
        ..NetworkItemStack::default()
    }
}

fn player_ledger(
    registry: Option<ItemRegistryEvent>,
    target: NetworkItemStack,
    cursor: NetworkItemStack,
) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    if let Some(registry) = registry {
        ledger.apply_registry(&registry);
    }
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = target;
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::default(),
    }));
    ledger.apply(&cursor_content(cursor));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
}

fn storage_ledger(slot_count: usize) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&registry(vec![entry(
        6,
        "minecraft:apple",
        Some(64),
        true,
        false,
    )]));
    ledger.apply(&cursor_content(stack(6, 33, 33)));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    let mut slots = vec![NetworkItemStack::default(); slot_count];
    slots[2] = stack(6, 60, 60);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(7),
            slot_type: Some(CONTAINER_NAME_LEVEL_ENTITY),
            dynamic_id: Some(91),
        },
        slots: slots.into(),
        storage_item: NetworkItemStack::default(),
    }));
    ledger
}

fn cursor_content(stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(-1),
            slot_type: Some(CONTAINER_NAME_CURSOR),
            dynamic_id: None,
        },
        slots: Arc::from([stack]),
        storage_item: NetworkItemStack::default(),
    })
}

fn response(request_id: i32, status: StackResponseStatus) -> InventoryEvent {
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status,
            request_id,
            containers: Arc::from([]),
        }]),
    })
}

fn accepted_response(request_id: i32) -> InventoryEvent {
    accepted_response_with_ids(request_id, 61, 34)
}

fn accepted_response_with_ids(
    request_id: i32,
    destination_id: i32,
    source_id: i32,
) -> InventoryEvent {
    let correction = |container, slot, count, item_stack_id| StackResponseContainer {
        container,
        slots: Arc::from([StackResponseSlot {
            slot,
            hotbar_slot: slot,
            count,
            item_stack_id,
            custom_name: Arc::from(""),
            filtered_custom_name: Arc::from(""),
            durability_correction: 0,
        }]),
    };
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([
                correction(
                    ContainerIdentity {
                        window_id: None,
                        slot_type: Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                        dynamic_id: None,
                    },
                    0,
                    64,
                    destination_id,
                ),
                correction(
                    ContainerIdentity {
                        window_id: None,
                        slot_type: Some(CONTAINER_NAME_CURSOR),
                        dynamic_id: None,
                    },
                    0,
                    29,
                    source_id,
                ),
            ]),
        }]),
    })
}

fn apple_registry() -> ItemRegistryEvent {
    registry(vec![entry(6, "minecraft:apple", Some(64), true, false)])
}

#[test]
fn occupied_primary_moves_only_free_capacity_and_retains_both_identities() {
    let target = ten_zero_stack(6, 60, 60);
    let cursor = stack(6, 33, 33);
    let mut ledger = player_ledger(Some(apple_registry()), target.clone(), cursor.clone());
    ledger.slot_overlays[0] = Some(StackResponseOverlay::default());
    ledger.cursor_overlay = Some(StackResponseOverlay::default());

    assert_eq!(ledger.begin_click(0), Ok(-3));
    assert!(matches!(
        ledger.pending.as_ref().map(|pending| &pending.action),
        Some(StackRequestAction::Place {
            amount: 4,
            source: protocol::StackRequestSlot {
                stack_network_id: 33,
                ..
            },
            destination: protocol::StackRequestSlot {
                stack_network_id: 60,
                ..
            },
        })
    ));
    let destination = ledger.displayed_stack(0).unwrap();
    assert_eq!((destination.count, destination.stack_network_id), (64, 60));
    assert_eq!(destination.extra_data, target.extra_data);
    let source = ledger.cursor_stack().unwrap();
    assert_eq!((source.count, source.stack_network_id), (29, 33));
    assert_eq!(source.extra_data, cursor.extra_data);
    assert_eq!(
        ledger.presented_slot_overlay(0),
        Some(&StackResponseOverlay::default())
    );
    assert_eq!(
        ledger.cursor_overlay(),
        Some(&StackResponseOverlay::default())
    );
}

#[test]
fn explicit_occupied_counts_reject_zero_overflow_and_full_targets_atomically() {
    let mut place = player_ledger(Some(apple_registry()), stack(6, 60, 60), stack(6, 33, 33));
    for amount in [0, 5, 34, 256] {
        assert_eq!(
            place.begin_place_count(0, amount),
            Err(InventoryGestureError::InvalidRequest)
        );
        assert_eq!(place.next_request_id, -3);
        assert!(place.pending.is_none());
    }

    let mut take = player_ledger(Some(apple_registry()), stack(6, 33, 33), stack(6, 60, 60));
    assert_eq!(take.begin_take_count(0, 4), Ok(-3));
    assert_eq!(take.displayed_stack(0).unwrap().count, 29);
    assert_eq!(take.cursor_stack().unwrap().count, 64);

    let mut full = player_ledger(Some(apple_registry()), stack(6, 60, 64), stack(6, 33, 1));
    assert_eq!(
        full.begin_click(0),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(full.next_request_id, -3);
    assert!(full.pending.is_none());
}

#[test]
fn exact_capacity_one_sixteen_and_sixty_four_rules_are_used() {
    let mut capacity_one = player_ledger(
        Some(registry(vec![entry(
            7,
            "minecraft:water_bucket",
            None,
            false,
            true,
        )])),
        stack(7, 60, 1),
        stack(7, 33, 1),
    );
    assert_eq!(
        capacity_one.begin_click(0),
        Err(InventoryGestureError::InvalidRequest)
    );

    for (network_id, identifier, capacity, negotiated) in [
        (8, "minecraft:bucket", 16, None),
        (6, "minecraft:apple", 64, Some(64)),
    ] {
        let component_based = negotiated.is_some();
        let canonical_empty = negotiated.is_none();
        let registry = registry(vec![entry(
            network_id,
            identifier,
            negotiated,
            component_based,
            canonical_empty,
        )]);
        let mut ledger = player_ledger(
            Some(registry),
            stack(network_id, 60, capacity - 1),
            stack(network_id, 33, 2),
        );
        assert_eq!(ledger.begin_click(0), Ok(-3));
        assert_eq!(ledger.displayed_stack(0).unwrap().count, capacity);
        assert_eq!(ledger.cursor_stack().unwrap().count, 1);
    }
}

#[test]
fn unsupported_stack_shapes_never_guess_a_merge_rule() {
    let cases = [
        (None, stack(6, 60, 60), stack(6, 33, 3)),
        (Some(apple_registry()), stack(7, 60, 60), stack(7, 33, 3)),
        (
            Some(apple_registry()),
            NetworkItemStack {
                metadata: 1,
                ..stack(6, 60, 60)
            },
            stack(6, 33, 3),
        ),
        (
            Some(apple_registry()),
            NetworkItemStack {
                block_runtime_id: 1,
                ..stack(6, 60, 60)
            },
            stack(6, 33, 3),
        ),
        (
            Some(apple_registry()),
            stack(6, 60, 60),
            NetworkItemStack {
                extra_data: Arc::from([1]),
                ..stack(6, 33, 3)
            },
        ),
    ];
    for (registry, target, cursor) in cases {
        let mut ledger = player_ledger(registry, target, cursor);
        assert_eq!(
            ledger.begin_click(0),
            Err(InventoryGestureError::InvalidRequest)
        );
        assert_eq!(ledger.next_request_id, -3);
        assert!(ledger.pending.is_none());
    }

    let unsupported_component = registry(vec![entry(6, "minecraft:apple", None, true, true)]);
    let mut ledger = player_ledger(
        Some(unsupported_component),
        stack(6, 60, 60),
        stack(6, 33, 3),
    );
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::InvalidRequest)
    );

    let mut unknown_version_entry = entry(6, "minecraft:apple", Some(64), true, false);
    unknown_version_entry.version = ItemRegistryVersion::Unknown(99);
    let mut unknown_version = player_ledger(
        Some(registry(vec![unknown_version_entry])),
        stack(6, 60, 60),
        stack(6, 33, 3),
    );
    assert_eq!(
        unknown_version.begin_click(0),
        Err(InventoryGestureError::InvalidRequest)
    );

    let mut meaningful_overlay =
        player_ledger(Some(apple_registry()), stack(6, 60, 60), stack(6, 33, 3));
    meaningful_overlay.slot_overlays[0] = Some(StackResponseOverlay {
        custom_name: Some(Arc::from("named")),
        ..StackResponseOverlay::default()
    });
    assert_eq!(
        meaningful_overlay.begin_click(0),
        Err(InventoryGestureError::InvalidRequest)
    );
}

#[test]
fn equivalent_registry_repeats_preserve_pending_but_replacement_requires_resync() {
    for admitted in [false, true] {
        let original = apple_registry();
        let mut ledger = player_ledger(Some(original.clone()), stack(6, 60, 60), stack(6, 33, 33));
        let request = ledger.begin_click(0).unwrap();
        if admitted {
            assert!(ledger.mark_transport_enqueued(10));
        }
        ledger.apply_registry(&original);
        assert_eq!(ledger.pending_request_id(), Some(request));

        let mut harmless = original.entries.to_vec();
        harmless[0].component_digest = [99; 32];
        ledger.apply_registry(&registry(harmless));
        assert_eq!(ledger.pending_request_id(), Some(request));
        assert!(!ledger.resync_required());

        ledger.apply_registry(&registry(vec![entry(
            6,
            "minecraft:stick",
            Some(64),
            true,
            false,
        )]));
        assert_eq!(ledger.pending_request_id(), None);
        assert!(ledger.resync_required());
        ledger.apply(&response(request, StackResponseStatus::Accepted));
        assert_eq!(ledger.pending_request_id(), None);
    }
}

#[test]
fn unrelated_registry_addition_preserves_pending_merge_authority() {
    let original = apple_registry();
    let mut ledger = player_ledger(Some(original.clone()), stack(6, 60, 60), stack(6, 33, 33));
    let request = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(10));

    let mut entries = original.entries.to_vec();
    entries.push(entry(8, "minecraft:bucket", None, false, true));
    ledger.apply_registry(&registry(entries));

    assert_eq!(ledger.pending_request_id(), Some(request));
    assert!(!ledger.resync_required());
}

#[test]
fn capacity_only_change_rebinds_future_merges_without_invalidating_idle_cells() {
    let mut ledger = player_ledger(Some(apple_registry()), stack(6, 60, 60), stack(6, 33, 3));
    ledger.apply_registry(&registry(vec![entry(
        6,
        "minecraft:apple",
        Some(16),
        true,
        false,
    )]));
    assert!(!ledger.resync_required());
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::InvalidRequest)
    );

    for admitted in [false, true] {
        let mut pending = player_ledger(Some(apple_registry()), stack(6, 60, 60), stack(6, 33, 3));
        pending.begin_click(0).unwrap();
        if admitted {
            assert!(pending.mark_transport_enqueued(10));
        }
        pending.apply_registry(&registry(vec![entry(
            6,
            "minecraft:apple",
            Some(16),
            true,
            false,
        )]));
        assert_eq!(pending.pending_request_id(), None);
        assert_eq!(pending.resync_required(), admitted);
    }
}

#[test]
fn unsupported_identifier_rebind_still_invalidates_retained_cells() {
    let original = registry(vec![entry(91, "minecraft:custom_a", None, true, false)]);
    let mut ledger = player_ledger(Some(original), stack(91, 60, 1), stack(91, 33, 1));
    ledger.apply_registry(&registry(vec![entry(
        91,
        "minecraft:custom_b",
        None,
        true,
        false,
    )]));
    assert!(ledger.resync_required());
}

#[test]
fn reused_runtime_id_requires_restated_cells_before_using_new_binding() {
    let mut ledger = player_ledger(Some(apple_registry()), stack(6, 60, 60), stack(6, 33, 3));
    ledger.apply_registry(&registry(vec![entry(
        6,
        "minecraft:bucket",
        None,
        false,
        true,
    )]));
    assert!(ledger.resync_required());
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::ResyncRequired)
    );

    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = stack(6, 70, 15);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::default(),
    }));
    assert!(ledger.resync_required());
    ledger.apply(&cursor_content(stack(6, 71, 1)));
    assert!(!ledger.resync_required());
    assert_eq!(ledger.begin_click(0), Ok(-3));
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 16);
    assert!(ledger.cursor_stack().is_none());
}

#[test]
fn known_incompatible_primary_stays_swap_and_explicit_merge_is_rejected() {
    let items = registry(vec![
        entry(6, "minecraft:apple", Some(64), true, false),
        entry(8, "minecraft:bucket", None, false, true),
    ]);
    let mut click = player_ledger(Some(items.clone()), stack(8, 60, 1), stack(6, 33, 3));
    assert_eq!(click.begin_click(0), Ok(-3));
    assert!(matches!(
        click.pending.as_ref().map(|pending| &pending.action),
        Some(StackRequestAction::Swap { .. })
    ));

    let mut explicit = player_ledger(Some(items), stack(8, 60, 1), stack(6, 33, 3));
    assert_eq!(
        explicit.begin_place_count(0, 1),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert!(explicit.pending.is_none());

    let mut unregistered = player_ledger(None, stack(8, 60, 1), stack(6, 33, 3));
    assert_eq!(unregistered.begin_click(0), Ok(-3));
    assert!(matches!(
        unregistered.pending.as_ref().map(|pending| &pending.action),
        Some(StackRequestAction::Swap { .. })
    ));
}

#[test]
fn accepted_and_rejected_occupied_merges_reconcile_or_rollback() {
    let target = stack(6, 60, 60);
    let cursor = stack(6, 33, 33);
    let mut accepted = player_ledger(Some(apple_registry()), target.clone(), cursor.clone());
    let request = accepted.begin_click(0).unwrap();
    assert!(accepted.mark_transport_enqueued(10));
    accepted.apply(&accepted_response(request));
    assert_eq!(
        accepted
            .displayed_stack(0)
            .map(|stack| (stack.count, stack.stack_network_id)),
        Some((64, 61))
    );
    assert_eq!(
        accepted
            .cursor_stack()
            .map(|stack| (stack.count, stack.stack_network_id)),
        Some((29, 34))
    );

    let mut rejected = player_ledger(Some(apple_registry()), target.clone(), cursor.clone());
    let request = rejected.begin_click(0).unwrap();
    assert!(rejected.mark_transport_enqueued(10));
    rejected.apply(&response(request, StackResponseStatus::Rejected));
    assert_eq!(rejected.displayed_stack(0), Some(&target));
    assert_eq!(rejected.cursor_stack(), Some(&cursor));
    assert!(!rejected.resync_required());
}

#[test]
fn accepted_partial_occupied_merge_requires_distinct_authoritative_identities() {
    let mut ledger = player_ledger(Some(apple_registry()), stack(6, 60, 60), stack(6, 33, 33));
    let request = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&accepted_response_with_ids(request, 77, 77));

    assert!(ledger.resync_required());
    assert_eq!(ledger.pending_request_id(), None);
}

#[test]
fn occupied_merge_timeout_and_newer_full_update_do_not_commit_stale_prediction() {
    let target = stack(6, 60, 60);
    let cursor = stack(6, 33, 33);
    let mut timed_out = player_ledger(Some(apple_registry()), target.clone(), cursor.clone());
    timed_out.begin_click(0).unwrap();
    assert!(timed_out.mark_transport_enqueued(10));
    timed_out.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert_eq!(timed_out.displayed_stack(0), Some(&target));
    assert_eq!(timed_out.cursor_stack(), Some(&cursor));
    assert!(timed_out.resync_required());

    let mut raced = player_ledger(Some(apple_registry()), target, cursor.clone());
    let request = raced.begin_click(0).unwrap();
    assert!(raced.mark_transport_enqueued(10));
    let current = stack(6, 72, 58);
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = current.clone();
    raced.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::default(),
    }));
    raced.apply(&accepted_response(request));
    assert_eq!(raced.displayed_stack(0), Some(&current));
    assert_eq!(raced.cursor_stack(), Some(&cursor));
    assert!(raced.resync_required());
}

#[test]
fn both_storage_sizes_merge_and_new_storage_generation_rejects_old_response() {
    for slot_count in [SMALL_STORAGE_SLOT_COUNT, LARGE_STORAGE_SLOT_COUNT] {
        let mut ledger = storage_ledger(slot_count);
        let request = ledger.begin_storage_click(2).unwrap();
        assert_eq!(ledger.storage_stack(2).unwrap().count, 64);
        assert_eq!(ledger.cursor_stack().unwrap().count, 29);
        assert!(ledger.mark_transport_enqueued(10));
        ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(8),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            position: [2, 64, 2],
            runtime_entity_id: -1,
        }));
        ledger.apply(&response(request, StackResponseStatus::Accepted));
        assert!(ledger.resync_required());
    }
}

#[test]
fn session_replacement_drops_registry_rules() {
    let mut ledger = player_ledger(Some(apple_registry()), stack(6, 60, 60), stack(6, 33, 3));
    ledger.begin_session(2);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = stack(6, 60, 60);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::default(),
    }));
    ledger.apply(&cursor_content(stack(6, 33, 3)));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(3),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::InvalidRequest)
    );
}
