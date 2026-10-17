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

mod blocks;

/// A registry entry: DataDriven when `component_based` and Legacy otherwise, with the
/// `negotiated` stack size declared through `item_properties`.
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
        version: if component_based {
            ItemRegistryVersion::DataDriven
        } else {
            ItemRegistryVersion::Legacy
        },
        components: negotiated.map(|size| {
            Arc::new(protocol::ItemComponents {
                max_stack_size: Some(size),
                item_properties_declared: true,
                ..protocol::ItemComponents::default()
            })
        }),
        component_digest: [network_id as u8; 32],
        negotiated_max_stack_size: negotiated,
        canonical_empty_component_data: canonical_empty,
        item_tags: std::sync::Arc::from([]),
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

/// Restates the cursor through its fixed UI inventory slot.
fn cursor_content(stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
            slot: 0,
        },
        stack,
        storage_item: None,
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
fn occupied_primary_moves_only_free_capacity_and_stamps_sparse_identities() {
    let target = ten_zero_stack(6, 60, 60);
    let cursor = stack(6, 33, 33);
    let mut ledger = player_ledger(Some(apple_registry()), target.clone(), cursor.clone());
    ledger.set_confirmed_overlay(Cell::Inventory(0), StackResponseOverlay::default());
    ledger.set_confirmed_overlay(Cell::Cursor, StackResponseOverlay::default());

    assert_eq!(ledger.begin_click(0), Ok(-3));
    assert!(matches!(
        ledger.newest_action().as_ref(),
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
    assert_eq!((destination.count, destination.stack_network_id), (64, -3));
    assert_eq!(destination.extra_data, target.extra_data);
    let source = ledger.cursor_stack().unwrap();
    assert_eq!((source.count, source.stack_network_id), (29, -3));
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
        assert!(place.newest_request().is_none());
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
    assert!(full.newest_request().is_none());
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
fn unparsed_stack_size_data_cannot_enable_oversized_merges() {
    let registry = protocol::test_support::items::component_item_registry("test:gem", 6, 1, 40);
    let entry = &registry.entries[0];
    assert!(entry.components.is_none());
    assert!(!entry.canonical_empty_component_data);

    let target = stack(6, 60, 1);
    let cursor = stack(6, 33, 1);
    let mut ledger = player_ledger(Some(registry), target.clone(), cursor.clone());
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.displayed_stack(0), Some(&target));
    assert_eq!(ledger.cursor_stack(), Some(&cursor));
    assert_eq!(ledger.pending_request_count(), 0);
}

/// A wire-admitted root with duplicate component compounds and a hidden size-one declaration.
fn duplicate_component_registry() -> ItemRegistryEvent {
    let mut bytes = vec![10, 0, 10];
    bytes.push(u8::try_from("components".len()).unwrap());
    bytes.extend_from_slice(b"components");
    bytes.push(0);
    bytes.push(10);
    bytes.push(u8::try_from("components".len()).unwrap());
    bytes.extend_from_slice(b"components");
    bytes.push(10);
    bytes.push(u8::try_from("item_properties".len()).unwrap());
    bytes.extend_from_slice(b"item_properties");
    bytes.push(3);
    bytes.push(u8::try_from("max_stack_size".len()).unwrap());
    bytes.extend_from_slice(b"max_stack_size");
    bytes.extend_from_slice(&[2, 0, 0, 0]);
    protocol::test_support::items::component_item_registry_payload("test:gem", 6, bytes)
}

#[test]
fn duplicate_component_ancestors_cannot_enable_oversized_merges() {
    let registry = duplicate_component_registry();
    let entry = &registry.entries[0];
    assert!(entry.components.as_ref().unwrap().stack_capacity_ambiguous);
    assert!(entry.negotiated_max_stack_size.is_none());
    let target = stack(6, 60, 1);
    let cursor = stack(6, 33, 1);
    let mut ledger = player_ledger(Some(registry), target.clone(), cursor.clone());
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.displayed_stack(0), Some(&target));
    assert_eq!(ledger.cursor_stack(), Some(&cursor));
    assert_eq!(ledger.pending_request_count(), 0);
}

#[test]
fn unsupported_stack_shapes_never_guess_a_merge_rule_and_registry_shapes_bind_natively() {
    let cases = [
        (None, stack(6, 60, 60), stack(6, 33, 3)),
        (Some(apple_registry()), stack(7, 60, 60), stack(7, 33, 3)),
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
        assert!(ledger.newest_request().is_none());
    }

    // An empty component item and an unknown item version keep the item default, as the
    // native registry does.
    let empty_component = registry(vec![entry(6, "minecraft:apple", None, true, true)]);
    let mut ledger = player_ledger(Some(empty_component), stack(6, 60, 60), stack(6, 33, 3));
    assert_eq!(ledger.begin_click(0), Ok(-3));

    let mut unknown_version_entry = entry(6, "minecraft:apple", Some(64), true, false);
    unknown_version_entry.version = ItemRegistryVersion::Unknown(99);
    let mut unknown_version = player_ledger(
        Some(registry(vec![unknown_version_entry])),
        stack(6, 60, 60),
        stack(6, 33, 3),
    );
    assert_eq!(unknown_version.begin_click(0), Ok(-3));

    let mut meaningful_overlay =
        player_ledger(Some(apple_registry()), stack(6, 60, 60), stack(6, 33, 3));
    meaningful_overlay.set_confirmed_overlay(
        Cell::Inventory(0),
        StackResponseOverlay {
            custom_name: Some(Arc::from("named")),
            ..StackResponseOverlay::default()
        },
    );
    assert_eq!(meaningful_overlay.begin_click(0), Ok(-3));
    assert_eq!(meaningful_overlay.displayed_stack(0).unwrap().count, 3);
    assert_eq!(meaningful_overlay.cursor_stack().unwrap().count, 60);
    assert_eq!(
        meaningful_overlay
            .view()
            .get(Cell::Cursor)
            .unwrap()
            .overlay
            .as_ref()
            .unwrap()
            .custom_name
            .as_deref(),
        Some("named")
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
        click.newest_action().as_ref(),
        Some(StackRequestAction::Swap { .. })
    ));

    let mut explicit = player_ledger(Some(items), stack(8, 60, 1), stack(6, 33, 3));
    assert_eq!(
        explicit.begin_place_count(0, 1),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert!(explicit.newest_request().is_none());

    let mut unregistered = player_ledger(None, stack(8, 60, 1), stack(6, 33, 3));
    assert_eq!(unregistered.begin_click(0), Ok(-3));
    assert!(matches!(
        unregistered.newest_action().as_ref(),
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
fn occupied_merge_timeout_keeps_prediction_and_response_uses_historic_snapshot() {
    let target = stack(6, 60, 60);
    let cursor = stack(6, 33, 33);
    let mut timed_out = player_ledger(Some(apple_registry()), target.clone(), cursor.clone());
    timed_out.begin_click(0).unwrap();
    assert!(timed_out.mark_transport_enqueued(10));
    timed_out.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    // A timeout never rolls back: the server may still apply the merge.
    assert_eq!(
        timed_out.displayed_stack(0).map(|stack| stack.count),
        Some(64)
    );
    assert_eq!(timed_out.cursor_stack().map(|stack| stack.count), Some(29));
    assert_eq!(timed_out.confirmed_stack(Cell::Inventory(0)), Some(&target));
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
    // The accepted request corrects its own historic item, never replaying a
    // merge against the authoritative count that arrived during flight.
    assert_eq!(raced.displayed_stack(0).unwrap().count, 64);
    assert_eq!(raced.cursor_stack().unwrap().count, 29);
    assert!(!raced.resync_required());
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

/// Registry invalidation still needs authoritative content after a rejection.
#[test]
fn late_rejection_preserves_registry_identity_recovery() {
    let mut ledger = player_ledger(Some(apple_registry()), stack(6, 60, 60), stack(6, 33, 33));
    let request = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply_registry(&registry(vec![entry(
        6,
        "minecraft:stick",
        Some(64),
        true,
        false,
    )]));
    assert!(ledger.resync_required());
    ledger.apply(&response(request, StackResponseStatus::Rejected));
    assert!(
        ledger.resync_required(),
        "rejection did not restate item identity"
    );
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::ResyncRequired)
    );
}

/// Discarding a confirmed cursor needs recovery even when the request was rejected.
#[test]
fn late_rejection_preserves_discarded_personal_cursor_recovery() {
    for target_count in [0, 60] {
        let target = if target_count == 0 {
            NetworkItemStack::default()
        } else {
            stack(6, 60, target_count)
        };
        let mut ledger = player_ledger(Some(apple_registry()), target, stack(6, 33, 33));
        let request = ledger.begin_click(0).unwrap();
        assert!(ledger.mark_transport_enqueued(10));
        ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
            container: ContainerIdentity::window(2),
            window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
            server_initiated: true,
        }));
        assert_eq!(ledger.cursor_stack(), None);
        ledger.apply(&response(request, StackResponseStatus::Rejected));
        assert!(
            ledger.resync_required(),
            "the confirmed cursor was discarded"
        );
        assert_eq!(
            ledger.begin_world_drop(0, Some(1)),
            Err(InventoryGestureError::ResyncRequired)
        );
    }
}

/// A current screen response above item capacity is skipped rather than published.
#[test]
fn parity_requests_response_above_capacity_is_skipped() {
    if assets::facts::ItemFacts::builtin().is_err() {
        eprintln!("missing-fixture: builtin item facts carrier");
        return;
    }
    let original = stack(6, 10, 4);
    let mut ledger = player_ledger(
        Some(registry(vec![entry(
            6,
            "test:probe",
            Some(16),
            true,
            false,
        )])),
        original.clone(),
        NetworkItemStack::empty(),
    );
    assert_eq!(
        ledger
            .negotiated_item_entry(6)
            .and_then(super::registry::entry_capacity),
        Some(16)
    );
    let request = ledger
        .begin_drop(
            super::moves::DropSource::Target(super::gesture::InventoryTarget::Player(0)),
            Some(1),
        )
        .unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    let response = StackResponseContainer {
        container: ContainerIdentity {
            window_id: None,
            slot_type: Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
            dynamic_id: None,
        },
        slots: Arc::from([StackResponseSlot {
            slot: 0,
            hotbar_slot: 0,
            count: 17,
            item_stack_id: 11,
            custom_name: Arc::from(""),
            filtered_custom_name: Arc::from(""),
            durability_correction: 0,
        }]),
    };
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id: request,
            containers: Arc::from([response]),
        }]),
    }));
    assert_eq!(ledger.displayed_stack(0), Some(&original));
}

/// Equal names on compatible stacks may merge without losing their accepted overlay.
#[test]
fn parity_requests_matching_named_stacks_can_merge() {
    if assets::facts::ItemFacts::builtin().is_err() {
        eprintln!("missing-fixture: builtin item facts carrier");
        return;
    }
    let mut ledger = player_ledger(
        Some(registry(vec![entry(
            6,
            "test:probe",
            Some(64),
            true,
            false,
        )])),
        stack(6, 10, 4),
        stack(6, 11, 3),
    );
    let overlay = super::response::StackResponseOverlay {
        custom_name: Some(Arc::from("Original probe label")),
        filtered_custom_name: Some(Arc::from("Original probe label")),
        durability_correction: Some(0),
    };
    ledger.set_confirmed_overlay(super::cells::Cell::Inventory(0), overlay.clone());
    ledger.set_confirmed_overlay(super::cells::Cell::Cursor, overlay);
    ledger
        .begin_click(0)
        .expect("equal names do not make compatible items unmergeable");
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 7);
    assert!(ledger.cursor_stack().is_none());
}

/// Equal decoded custom-name compounds retain ordinary item identity during a merge.
#[test]
fn parity_requests_pass2_identical_named_nbt_can_merge() {
    assets::facts::ItemFacts::builtin().expect("required builtin facts fixture");
    let extra = protocol::edit_item_extra(
        &[0; 10],
        &protocol::ItemNbtEdit {
            custom_name: Some(Some("Original parity probe".to_owned())),
            ..Default::default()
        },
    )
    .expect("original valid named compound");
    assert_eq!(
        protocol::item_custom_name(&extra).as_deref(),
        Some("Original parity probe")
    );
    let mut target = stack(6, 10, 4);
    target.nbt_digest = Sha256::digest(&extra).into();
    target.extra_data = extra.clone();
    let mut cursor = target.clone();
    cursor.count = 3;
    cursor.stack_network_id = 11;
    assert_eq!(target.extra_data, cursor.extra_data);
    assert_eq!(target.nbt_digest, cursor.nbt_digest);
    let mut ledger = player_ledger(
        Some(registry(vec![entry(
            6,
            "test:probe",
            Some(64),
            true,
            false,
        )])),
        target,
        cursor,
    );
    ledger
        .begin_click(0)
        .expect("matching named item data can merge");
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 7);
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(
        protocol::item_custom_name(&ledger.displayed_stack(0).unwrap().extra_data).as_deref(),
        Some("Original parity probe")
    );
}

#[path = "merge_tests/rebase.rs"]
mod rebase;
