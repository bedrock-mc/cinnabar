use client_ui::ui_runtime::UiRuntime;
use protocol::*;

/// Applies an event through the same UI queue and drain used during gameplay.
fn commit(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    sequence: u64,
    event: InventoryEvent,
) {
    runtime
        .enqueue_inventory_event(player, 1, sequence, event)
        .unwrap();
    runtime.drain_pending_inventory(player);
}

/// Creates an empty content packet with the requested ordinary window and shape.
fn content(window: i32, name: Option<u8>, length: usize) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(window),
            slot_type: name,
            dynamic_id: None,
        },
        slots: vec![NetworkItemStack::empty(); length].into(),
        storage_item: NetworkItemStack::empty(),
    })
}

#[test]
fn foreign_content_cannot_hide_open_chest() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    commit(
        &mut runtime,
        &mut player,
        1,
        InventoryEvent::Authority(InventoryAuthority::Server),
    );
    commit(
        &mut runtime,
        &mut player,
        2,
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(7),
            window_type: WINDOW_TYPE_CONTAINER,
            position: [0; 3],
            runtime_entity_id: -1,
        }),
    );
    assert!(runtime.inventory_open());
    let generation = player.inventory.ledger().storage_generation();
    commit(
        &mut runtime,
        &mut player,
        3,
        content(6, Some(CONTAINER_NAME_LEVEL_ENTITY), 27),
    );
    assert_eq!(player.inventory.ledger().storage_generation(), generation);
    assert!(generation.is_some());
    let after_foreign = runtime.inventory_open();
    commit(&mut runtime, &mut player, 4, content(7, None, 27));
    assert_eq!(player.inventory.ledger().storage_slot_count(), Some(27));
    assert!(
        after_foreign && runtime.inventory_open(),
        "rejected foreign Content hid an active chest; later admitted bare Content did not restore it"
    );
}

#[test]
fn bare_content_positive_control_keeps_chest_open() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    commit(
        &mut runtime,
        &mut player,
        1,
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(7),
            window_type: WINDOW_TYPE_CONTAINER,
            position: [0; 3],
            runtime_entity_id: -1,
        }),
    );
    commit(&mut runtime, &mut player, 2, content(7, None, 27));
    assert!(runtime.inventory_open());
    assert_eq!(player.inventory.ledger().storage_slot_count(), Some(27));
}
