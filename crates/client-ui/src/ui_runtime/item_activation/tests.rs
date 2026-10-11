use super::*;
use protocol::{
    ContainerIdentity, InventoryContentEvent, InventoryEvent, ItemRegistryEvent, NetworkItemStack,
};

#[test]
fn totem_pending_world_commit_keeps_the_pre_consumption_snapshot() {
    let (mut runtime, mut player, _) = queued_player();
    runtime.retain_totem_snapshot(&mut player, 1, 4);
    assert!(
        runtime.item_activation.is_none(),
        "publication still owns admission"
    );
    runtime.drain_pending_inventory(&mut player);
    runtime.activate_totem(&mut player, 1, 4, 100).unwrap();
    assert_eq!(runtime.item_activation.as_ref().unwrap().metadata, 7);
}

#[test]
fn totem_pending_snapshot_is_retired_with_its_session() {
    let (mut runtime, mut player, _) = queued_player();
    runtime.retain_totem_snapshot(&mut player, 1, 4);
    runtime.begin_session(2);
    assert!(runtime.pending_totem_snapshots.is_empty());
    runtime.retain_totem_snapshot(&mut player, 1, 5);
    assert!(runtime.pending_totem_snapshots.is_empty());
}

#[test]
fn totem_equipment_only_main_hand_keeps_the_activation_prefix() {
    equipment_only_prefix(protocol::ActorHandedness::Right);
}

#[test]
fn totem_equipment_only_offhand_keeps_the_activation_prefix() {
    equipment_only_prefix(protocol::ActorHandedness::Left);
}

/// Sends equipment and consumption around an activation without inventory-cell packets.
fn equipment_only_prefix(hand: protocol::ActorHandedness) {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let registry = protocol::vanilla_item_registry();
    let id = registry
        .iter()
        .find(|entry| entry.identifier.as_ref() == TOTEM_IDENTIFIER)
        .unwrap()
        .network_id;
    player
        .inventory
        .enqueue_item_registry_event(1, 1, ItemRegistryEvent { entries: registry })
        .unwrap();
    let equipment = protocol::EquipmentEvent {
        actor_runtime_id: 42,
        stack: NetworkItemStack {
            network_id: id,
            count: 1,
            metadata: 13,
            ..NetworkItemStack::empty()
        },
        inventory_slot: 0,
        selected_slot: 0,
        window_id: if hand == protocol::ActorHandedness::Left {
            119
        } else {
            0
        },
        handedness: Some(hand),
    };
    runtime.retain_local_selected_equipment(&mut player, 2, equipment.clone());
    runtime.retain_totem_snapshot(&mut player, 1, 3);
    runtime.retain_local_selected_equipment(
        &mut player,
        4,
        protocol::EquipmentEvent {
            stack: NetworkItemStack::empty(),
            ..equipment
        },
    );
    runtime.drain_pending_inventory(&mut player);
    runtime.activate_totem(&mut player, 1, 3, 100).unwrap();
    assert_eq!(
        runtime.item_activation.as_ref().unwrap().metadata,
        13,
        "{hand:?}"
    );
}

#[test]
fn totem_motion_offsets_preserve_rounded_random_endpoints() {
    assert_eq!(sampled_offset(0), -1);
    assert_eq!(sampled_offset(u32::MAX), 1);
    assert_eq!(sampled_offset(u32::MAX / 2), 0);
}

#[test]
fn totem_stall_advances_only_bounded_client_time_and_backward_time_is_inert() {
    let (mut runtime, mut player, _) = queued_player();
    runtime.activate_totem(&mut player, 1, 4, 100).unwrap();
    runtime.advance_item_activation(3_100);
    assert_eq!(
        runtime.item_activation.as_ref().unwrap().progress(),
        Some(0.05)
    );
    runtime.advance_item_activation(100);
    runtime.advance_item_activation(3_100);
    assert_eq!(
        runtime.item_activation.as_ref().unwrap().progress(),
        Some(0.05)
    );
    runtime.advance_item_activation(3_125);
    assert_eq!(
        runtime.item_activation.as_ref().unwrap().progress(),
        Some(0.0625)
    );
}

#[test]
fn totem_activation_before_frame_advance_excludes_the_preceding_frame_gap() {
    for (previous_frame, activation_frame, partial_tick) in [(100, 200, 0), (125, 175, 25)] {
        let (mut runtime, mut player, _) = queued_player();
        runtime.advance_item_activation(100);
        runtime.advance_item_activation(previous_frame);
        runtime
            .activate_totem(&mut player, 1, 4, activation_frame)
            .unwrap();
        runtime.advance_item_activation(activation_frame);
        assert_eq!(
            runtime.item_activation.as_ref().unwrap().elapsed_millis,
            partial_tick,
            "only the current partial tick belongs to a newly admitted activation"
        );
        let expires_at =
            activation_frame + ACTIVATION_TICKS * MILLIS_PER_SERVER_TICK - partial_tick;
        for now in (activation_frame + 50..expires_at).step_by(50) {
            runtime.advance_item_activation(now);
        }
        runtime.advance_item_activation(expires_at - 1);
        assert!(runtime.item_activation.is_some());
        runtime.advance_item_activation(expires_at);
        assert!(runtime.item_activation.is_none());
    }
}

#[test]
fn totem_uses_the_current_client_partial_tick_when_triggered_between_ticks() {
    let (mut runtime, mut player, _) = queued_player();
    runtime.advance_item_activation(100);
    runtime.advance_item_activation(125);
    runtime.activate_totem(&mut player, 1, 4, 125).unwrap();
    assert_eq!(
        runtime.item_activation.as_ref().unwrap().progress(),
        Some(0.0125)
    );
    runtime.advance_item_activation(150);
    assert_eq!(
        runtime.item_activation.as_ref().unwrap().progress(),
        Some(0.025)
    );
    runtime.activate_totem(&mut player, 1, 6, 150).unwrap();
    assert_eq!(
        runtime.item_activation.as_ref().unwrap().progress(),
        Some(0.0)
    );
}

/// Establishes an item and a later consumption in the same ingress drain.
fn queued_player() -> (UiRuntime, player_state::PlayerState, i32) {
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let registry = protocol::vanilla_item_registry();
    let id = registry
        .iter()
        .find(|entry| entry.identifier.as_ref() == TOTEM_IDENTIFIER)
        .unwrap()
        .network_id;
    player
        .inventory
        .enqueue_item_registry_event(1, 1, ItemRegistryEvent { entries: registry })
        .unwrap();
    for (sequence, window_id, metadata) in [(2, 0, 3), (3, 119, 7), (5, 119, 0)] {
        let items: Arc<[NetworkItemStack]> = Arc::from([NetworkItemStack {
            network_id: if sequence == 5 { 0 } else { id },
            metadata,
            stack_network_id: sequence as i32,
            count: if sequence == 5 { 0 } else { 1 },
            block_runtime_id: 0,
            nbt_digest: [0; 32],
            extra_data: Arc::from([]),
        }]);
        player
            .inventory
            .enqueue_inventory_event(
                1,
                sequence,
                InventoryEvent::Content(InventoryContentEvent {
                    container: ContainerIdentity {
                        window_id: Some(window_id),
                        slot_type: None,
                        dynamic_id: None,
                    },
                    slots: items,
                    storage_item: NetworkItemStack {
                        network_id: 0,
                        metadata: 0,
                        stack_network_id: 0,
                        count: 0,
                        block_runtime_id: 0,
                        nbt_digest: [0; 32],
                        extra_data: Arc::from([]),
                    },
                }),
            )
            .unwrap();
    }
    (UiRuntime::new(1), player, id)
}

#[test]
fn totem_snapshots_offhand_before_consumption_and_survives_inventory_removal() {
    let (mut runtime, mut player, _) = queued_player();
    runtime.activate_totem(&mut player, 1, 4, 100).unwrap();
    assert_eq!(runtime.item_activation.as_ref().unwrap().metadata, 7);
    runtime.drain_pending_inventory(&mut player);
    assert!(
        player
            .inventory
            .ledger()
            .target_stack(InventoryTarget::Offhand)
            .is_none()
    );
    let activation = runtime.item_activation.as_ref().unwrap();
    assert_eq!(activation.metadata, 7);
    assert_eq!(activation.progress(), Some(0.0));
    for now in (200..=1_100).step_by(100) {
        runtime.advance_item_activation(now);
    }
    assert_eq!(
        runtime.item_activation.as_ref().unwrap().progress(),
        Some(0.5)
    );
    for now in (1_200..=2_000).step_by(100) {
        runtime.advance_item_activation(now);
    }
    runtime.advance_item_activation(2_099);
    assert!(
        runtime
            .item_activation
            .as_ref()
            .unwrap()
            .progress()
            .is_some()
    );
    runtime.advance_item_activation(2_100);
    assert!(runtime.item_activation.is_none());
}

#[test]
fn totem_retrigger_uses_main_hand_and_session_replacement_retires_it() {
    let (mut runtime, mut player, _) = queued_player();
    runtime.activate_totem(&mut player, 1, 4, 100).unwrap();
    runtime.activate_totem(&mut player, 1, 6, 1_000).unwrap();
    let activation = runtime.item_activation.as_ref().unwrap();
    assert_eq!(activation.metadata, 3);
    assert_eq!(activation.progress(), Some(0.0));
    runtime.begin_session(2);
    assert!(runtime.item_activation.is_none());
}

#[test]
fn totem_activation_without_an_equipped_totem_replaces_the_previous_image() {
    let (mut runtime, mut player, _) = queued_player();
    runtime.activate_totem(&mut player, 1, 4, 100).unwrap();
    player
        .inventory
        .enqueue_inventory_event(
            1,
            7,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity {
                    window_id: Some(0),
                    slot_type: None,
                    dynamic_id: None,
                },
                slots: Arc::from([NetworkItemStack {
                    network_id: 0,
                    metadata: 0,
                    stack_network_id: 0,
                    count: 0,
                    block_runtime_id: 0,
                    nbt_digest: [0; 32],
                    extra_data: Arc::from([]),
                }]),
                storage_item: NetworkItemStack {
                    network_id: 0,
                    metadata: 0,
                    stack_network_id: 0,
                    count: 0,
                    block_runtime_id: 0,
                    nbt_digest: [0; 32],
                    extra_data: Arc::from([]),
                },
            }),
        )
        .unwrap();
    runtime.activate_totem(&mut player, 1, 8, 1_000).unwrap();
    assert!(runtime.item_activation.is_none());
}

#[test]
fn totem_rejected_activation_does_not_drain_inventory_or_replace_the_snapshot() {
    let (mut runtime, mut player, _) = queued_player();
    assert!(runtime.activate_totem(&mut player, 2, 4, 100).is_err());
    assert!(runtime.item_activation.is_none());
    runtime.activate_totem(&mut player, 1, 4, 100).unwrap();
    assert!(runtime.activate_totem(&mut player, 1, 4, 101).is_err());
    assert_eq!(runtime.item_activation.as_ref().unwrap().metadata, 7);
}
