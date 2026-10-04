use std::sync::Arc;

use protocol::{ContainerIdentity, InventorySlotEvent, ItemRegistryEvent, SlotIdentity};

use super::*;

/// Creates an independent UI and domain owner at the observation session.
fn runtime(player_runtime: &mut player_state::PlayerState, enabled: bool) -> UiRuntime {
    *player_runtime = player_state::PlayerState::new(7);
    let mut runtime = UiRuntime::new(7);
    runtime.use_on_identity_evidence = UseOnIdentityEvidence::new(enabled, 7);
    player_runtime.inventory.set_local_selected_slot(0);
    runtime
}

fn registry() -> InventoryAuthorityEvent {
    InventoryAuthorityEvent::Registry(ItemRegistryEvent {
        entries: Arc::from([ItemRegistryEntry {
            identifier: Arc::from(TARGET),
            network_id: 2,
            component_based: false,
            version: ItemRegistryVersion::None,
            component_digest: [7; 32],
            negotiated_max_stack_size: None,
            canonical_empty_component_data: true,
            item_tags: std::sync::Arc::from([]),
        }]),
    })
}

fn slot(count: u16) -> InventoryAuthorityEvent {
    let extra_data: Arc<[u8]> = Arc::from(&b"private-payload-token-chat-name"[..]);
    InventoryAuthorityEvent::Inventory(InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 0,
        },
        stack: NetworkItemStack {
            network_id: 2,
            metadata: 3,
            stack_network_id: 41,
            count,
            block_runtime_id: i32::from_ne_bytes(0x87654321_u32.to_ne_bytes()),
            nbt_digest: Sha256::digest(&extra_data).into(),
            extra_data,
        },
        storage_item: None,
    }))
}

fn push(
    player_runtime: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    sequence: u64,
    event: InventoryAuthorityEvent,
) {
    match event {
        InventoryAuthorityEvent::Registry(event) => runtime
            .enqueue_item_registry_event(player_runtime, 7, sequence, event)
            .unwrap(),
        InventoryAuthorityEvent::Inventory(event) => runtime
            .enqueue_inventory_event(player_runtime, 7, sequence, event)
            .unwrap(),
    }
    runtime.drain_pending_inventory(player_runtime);
}

#[test]
fn normal_transaction_observes_the_final_stack_at_its_single_fifo_sequence() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = runtime(&mut player_runtime, true);
    push(&mut player_runtime, &mut runtime, 1, registry());
    let InventoryAuthorityEvent::Inventory(InventoryEvent::Slot(update)) = slot(64) else {
        panic!()
    };
    push(
        &mut player_runtime,
        &mut runtime,
        2,
        InventoryAuthorityEvent::Inventory(InventoryEvent::Transaction(
            protocol::InventoryTransactionEvent {
                slots: Arc::from([update]),
                skipped_actions: 0,
            },
        )),
    );
    let (sequence, identity) = runtime.use_on_identity_evidence.stack_sources[0]
        .as_ref()
        .unwrap();
    assert_eq!(*sequence, 2);
    assert_eq!(identity.count, 64);
    assert_eq!(identity.stack_network_id, 41);
}

#[test]
fn cloned_ui_preserves_evidence_fifo_deduplication_and_full_quota() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut original = runtime(&mut player_runtime, true);
    push(&mut player_runtime, &mut original, 1, registry());
    push(&mut player_runtime, &mut original, 2, slot(37));
    let mut snapshot_player = player_runtime.clone();
    let mut snapshot = original.clone();
    let evidence = &snapshot.use_on_identity_evidence;
    assert!(evidence.enabled);
    assert_eq!(evidence.session, 7);
    assert_eq!(evidence.last_sequence, Some(2));
    assert_eq!(
        evidence.registry,
        original.use_on_identity_evidence.registry
    );
    assert_eq!(
        evidence.stack_sources,
        original.use_on_identity_evidence.stack_sources
    );
    assert_eq!(
        serde_json::to_value(&evidence.rows).unwrap(),
        serde_json::to_value(&original.use_on_identity_evidence.rows).unwrap()
    );
    assert!(!snapshot.use_on_identity_evidence.admit_sequence(7, 2));
    push(&mut snapshot_player, &mut snapshot, 3, slot(37));
    assert_eq!(
        snapshot.use_on_identity_evidence.rows.len(),
        1,
        "clone must not log an already observed identity again"
    );
    push(&mut snapshot_player, &mut snapshot, 4, slot(36));
    assert_eq!(snapshot.use_on_identity_evidence.rows.len(), 2);
    assert_eq!(original.use_on_identity_evidence.rows.len(), 1);
    for count in 1..=12 {
        push(
            &mut player_runtime,
            &mut original,
            u64::from(count) + 2,
            slot(count),
        );
    }
    let mut full = original.clone();
    assert_eq!(full.use_on_identity_evidence.rows.len(), MAX_ROWS);
    assert_eq!(
        full.use_on_identity_evidence.last_sequence,
        original.use_on_identity_evidence.last_sequence
    );
    assert!(!full.use_on_identity_evidence.admit_sequence(7, 100));
    player_runtime.begin_session(8);
    full.begin_session(8);
    assert!(full.use_on_identity_evidence.rows.is_empty());
    assert_eq!(original.use_on_identity_evidence.rows.len(), MAX_ROWS);
}

#[test]
fn disabled_observation_has_no_retained_rows_or_sources() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = runtime(&mut player_runtime, false);
    push(&mut player_runtime, &mut runtime, 1, registry());
    push(&mut player_runtime, &mut runtime, 2, slot(37));
    assert!(runtime.use_on_identity_evidence.rows.is_empty());
    assert!(runtime.use_on_identity_evidence.registry.is_none());
    assert!(
        runtime
            .use_on_identity_evidence
            .stack_sources
            .iter()
            .all(Option::is_none)
    );
}

#[test]
fn ordered_drain_correlates_both_registry_and_stack_arrival_orders() {
    let mut player_runtime = player_state::PlayerState::new(1);

    for stack_first in [false, true] {
        let mut runtime = runtime(&mut player_runtime, true);
        let (first, second) = if stack_first {
            (slot(37), registry())
        } else {
            (registry(), slot(37))
        };
        push(&mut player_runtime, &mut runtime, 1, first);
        assert!(runtime.use_on_identity_evidence.rows.is_empty());
        push(&mut player_runtime, &mut runtime, 2, second);
        let row = &runtime.use_on_identity_evidence.rows[0];
        assert_eq!(row.session_generation, 7);
        assert_eq!(row.observed_fifo_sequence, 2);
        assert_eq!(
            row.stack_observed_fifo_sequence,
            if stack_first { 1 } else { 2 }
        );
        assert_eq!(
            row.registry_observed_fifo_sequence,
            if stack_first { 2 } else { 1 }
        );
        assert_eq!(row.identity.item_version_wire_value, 2);
        assert_eq!(row.identity.stack.block_runtime_bits, 0x87654321);
        assert_eq!(row.identity.stack.count, 37);
        assert_eq!(row.identity.stack.metadata, 3);
        assert_eq!(row.identity.stack.stack_network_id, 41);
        push(&mut player_runtime, &mut runtime, 3, slot(37));
        push(&mut player_runtime, &mut runtime, 4, registry());
        assert_eq!(runtime.use_on_identity_evidence.rows.len(), 1);
    }
}

#[test]
fn full_inventory_content_establishes_selected_wire_stack_provenance() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = runtime(&mut player_runtime, true);
    push(&mut player_runtime, &mut runtime, 1, registry());
    let InventoryAuthorityEvent::Inventory(InventoryEvent::Slot(event)) = slot(37) else {
        unreachable!()
    };
    push(
        &mut player_runtime,
        &mut runtime,
        2,
        InventoryAuthorityEvent::Inventory(InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: Arc::from([event.stack]),
                storage_item: NetworkItemStack::default(),
            },
        )),
    );
    assert_eq!(runtime.use_on_identity_evidence.rows.len(), 1);
    assert_eq!(
        runtime.use_on_identity_evidence.rows[0].stack_observed_fifo_sequence,
        2
    );
}

#[test]
fn deduplicated_rows_are_capped_and_new_session_clears_every_identity() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = runtime(&mut player_runtime, true);
    push(&mut player_runtime, &mut runtime, 1, registry());
    for count in 1..=12 {
        push(
            &mut player_runtime,
            &mut runtime,
            u64::from(count) + 1,
            slot(count),
        );
    }
    assert_eq!(runtime.use_on_identity_evidence.rows.len(), MAX_ROWS);
    player_runtime.begin_session(8);
    runtime.begin_session(8);
    let evidence = &runtime.use_on_identity_evidence;
    assert_eq!(evidence.session, 8);
    assert!(evidence.rows.is_empty());
    assert!(evidence.registry.is_none());
    assert!(evidence.last_sequence.is_none());
    assert!(evidence.stack_sources.iter().all(Option::is_none));
    assert!(evidence.enabled);
}

#[test]
fn stale_session_or_fifo_cannot_replace_observed_identity() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = runtime(&mut player_runtime, true);
    push(&mut player_runtime, &mut runtime, 1, registry());
    push(&mut player_runtime, &mut runtime, 2, slot(37));
    runtime.observe_use_on_identity(&player_runtime, 8, 3, &registry());
    runtime.observe_use_on_identity(&player_runtime, 7, 1, &registry());
    assert_eq!(runtime.use_on_identity_evidence.last_sequence, Some(2));
    assert_eq!(
        runtime
            .use_on_identity_evidence
            .registry
            .as_ref()
            .unwrap()
            .0,
        1
    );
    assert_eq!(runtime.use_on_identity_evidence.rows.len(), 1);
}

#[test]
fn pending_selection_and_mismatched_wire_stack_are_not_authority_rows() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = runtime(&mut player_runtime, true);
    player_runtime
        .inventory
        .queue_local_hotbar_selection(1, None);
    player_runtime
        .inventory
        .queue_local_hotbar_selection(0, None);
    push(&mut player_runtime, &mut runtime, 1, registry());
    push(&mut player_runtime, &mut runtime, 2, slot(37));
    assert!(runtime.use_on_identity_evidence.rows.is_empty());
    player_runtime.inventory.clear_pending_hotbar_selection(0);
    // A changed ledger snapshot alone does not establish new wire provenance.
    let InventoryAuthorityEvent::Inventory(event) = slot(12) else {
        unreachable!()
    };
    player_runtime.inventory.ledger_mut().apply(&event);
    runtime.observe_use_on_identity(&player_runtime, 7, 3, &registry());
    assert!(runtime.use_on_identity_evidence.rows.is_empty());
    push(&mut player_runtime, &mut runtime, 4, slot(12));
    assert_eq!(runtime.use_on_identity_evidence.rows.len(), 1);
}

#[test]
fn pending_inventory_prediction_and_required_recovery_are_not_authority_rows() {
    let mut player_runtime = player_state::PlayerState::new(1);

    use crate::ui_runtime::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE;
    use protocol::{
        CONTAINER_NAME_CURSOR, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    };

    let mut runtime = runtime(&mut player_runtime, true);
    player_runtime
        .inventory
        .queue_local_hotbar_selection(1, None);
    player_runtime
        .inventory
        .queue_local_hotbar_selection(0, None);
    push(&mut player_runtime, &mut runtime, 1, registry());
    push(&mut player_runtime, &mut runtime, 2, slot(37));
    player_runtime
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    player_runtime
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: Some(-1),
                slot_type: Some(CONTAINER_NAME_CURSOR),
                dynamic_id: None,
            },
            slots: Arc::from([NetworkItemStack::default()]),
            storage_item: NetworkItemStack::default(),
        }));
    assert!(
        player_runtime
            .inventory
            .ledger_mut()
            .request_personal_open(42)
    );
    assert!(
        player_runtime
            .inventory
            .ledger_mut()
            .mark_transport_enqueued(0)
    );
    player_runtime
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
    player_runtime
        .inventory
        .ledger_mut()
        .begin_click(0)
        .unwrap();
    assert!(
        player_runtime
            .inventory
            .ledger_mut()
            .pending_request_id()
            .is_some()
    );
    player_runtime.inventory.clear_pending_hotbar_selection(0);
    push(&mut player_runtime, &mut runtime, 3, registry());
    assert!(runtime.use_on_identity_evidence.rows.is_empty());
    assert!(
        player_runtime
            .inventory
            .ledger_mut()
            .mark_transport_enqueued(0)
    );
    player_runtime.inventory.ledger_mut().poll_timeout(u64::MAX);
    assert!(player_runtime.inventory.ledger_mut().resync_required());
    push(&mut player_runtime, &mut runtime, 4, registry());
    assert!(runtime.use_on_identity_evidence.rows.is_empty());
}

#[test]
fn invalid_stack_digest_and_missing_target_registry_cannot_emit_rows() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = runtime(&mut player_runtime, true);
    push(&mut player_runtime, &mut runtime, 1, registry());
    let InventoryAuthorityEvent::Inventory(InventoryEvent::Slot(mut event)) = slot(37) else {
        unreachable!()
    };
    event.stack.nbt_digest = [0; 32];
    push(
        &mut player_runtime,
        &mut runtime,
        2,
        InventoryAuthorityEvent::Inventory(InventoryEvent::Slot(event)),
    );
    assert!(runtime.use_on_identity_evidence.rows.is_empty());
    assert!(runtime.use_on_identity_evidence.stack_sources[0].is_none());
    push(
        &mut player_runtime,
        &mut runtime,
        3,
        InventoryAuthorityEvent::Registry(ItemRegistryEvent {
            entries: Arc::from([]),
        }),
    );
    push(&mut player_runtime, &mut runtime, 4, slot(37));
    assert!(runtime.use_on_identity_evidence.rows.is_empty());
    assert!(runtime.use_on_identity_evidence.registry.is_none());
}

#[test]
fn serialization_contains_only_fixed_identity_fields_and_normalized_digests() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = runtime(&mut player_runtime, true);
    push(&mut player_runtime, &mut runtime, 1, registry());
    push(&mut player_runtime, &mut runtime, 2, slot(37));
    let row = &runtime.use_on_identity_evidence.rows[0];
    let encoded = serde_json::to_string(row).unwrap();
    for excluded in [
        "private-payload",
        "token",
        "chat",
        "account",
        "address",
        "extra_data",
        "nbt",
        "outgoing",
        "confirmed",
    ] {
        assert!(!encoded.contains(excluded), "{excluded}");
    }
    let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(value["identifier"], TARGET);
    assert_eq!(value["identity"]["client_selected_slot"], 0);
    assert!(value["identity"].get("slot").is_none());
    assert_eq!(
        value["identity"]["normalized_component_sha256"],
        serde_json::to_value([7_u8; 32]).unwrap()
    );
    assert_eq!(
        value["identity"]["stack"]["normalized_extra_sha256"]
            .as_array()
            .unwrap()
            .len(),
        32
    );
}
