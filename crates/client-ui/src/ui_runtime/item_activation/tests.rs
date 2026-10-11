use super::*;
use protocol::{
    ContainerIdentity, InventoryContentEvent, InventoryEvent, ItemRegistryEvent, NetworkItemStack,
};

#[test]
fn totem_motion_offsets_preserve_rounded_random_endpoints() {
    assert_eq!(sampled_offset(0), -1);
    assert_eq!(sampled_offset(u32::MAX), 1);
    assert_eq!(sampled_offset(u32::MAX / 2), 0);
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
    assert_eq!(activation.progress(100), Some(0.0));
    assert_eq!(activation.progress(1_100), Some(0.5));
    assert!(activation.progress(2_099).is_some());
    assert_eq!(activation.progress(2_100), None);
}

#[test]
fn totem_retrigger_uses_main_hand_and_session_replacement_retires_it() {
    let (mut runtime, mut player, _) = queued_player();
    runtime.activate_totem(&mut player, 1, 4, 100).unwrap();
    runtime.activate_totem(&mut player, 1, 6, 1_000).unwrap();
    let activation = runtime.item_activation.as_ref().unwrap();
    assert_eq!(activation.metadata, 3);
    assert_eq!(activation.progress(1_000), Some(0.0));
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
