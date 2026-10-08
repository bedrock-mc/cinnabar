use super::*;
use protocol::{
    InventoryAuthority, InventoryContentEvent, InventoryEvent, NetworkItemStack,
    PLAYER_INVENTORY_SLOTS, PLAYER_INVENTORY_WINDOW_ID,
};

fn pickup() -> InventoryEvent {
    let batch: Vec<_> =
        include_str!("../../../../protocol/fixtures/inventory_transaction_pickup.hex")
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect();
    let packet = decode_batch(batch.into(), &BedrockSession { shield_item_id: 0 })
        .unwrap()
        .pop()
        .unwrap();
    let Some(WorldEvent::Inventory(event)) = into_world_event(packet, 0).unwrap() else {
        panic!()
    };
    event
}

#[test]
fn vanilla_pickup_replaces_closed_inventory_and_hud_before_the_next_drop() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let event = pickup();
    let [update] = event.slot_updates() else {
        panic!()
    };
    let slot = u8::try_from(update.identity.slot).unwrap();
    let mut slots = vec![NetworkItemStack::empty(); usize::from(PLAYER_INVENTORY_SLOTS)];
    slots[usize::from(slot)] = NetworkItemStack {
        count: 63,
        stack_network_id: 80,
        ..update.stack.clone()
    };
    let mut runtime = UiRuntime::new(1);
    player_runtime.inventory.set_local_selected_slot(slot);
    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            1,
            1,
            InventoryEvent::Authority(InventoryAuthority::Server),
        )
        .unwrap();
    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            1,
            2,
            InventoryEvent::Content(InventoryContentEvent {
                container: protocol::ContainerIdentity::window(PLAYER_INVENTORY_WINDOW_ID),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player_runtime);
    let before = runtime
        .inventory_ledger(&player_runtime)
        .authoritative_slot_revision(slot)
        .unwrap();
    runtime
        .enqueue_inventory_event(&mut player_runtime, 1, 3, event.clone())
        .unwrap();
    runtime.drain_pending_inventory(&mut player_runtime);
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .authoritative_slot_revision(slot),
        Some(before + 1)
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(slot),
        Some(&update.stack)
    );
    assert_eq!(
        player_runtime.selected_stack_snapshot().unwrap().state,
        crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Present(&update.stack)
    );
    let request = runtime
        .inventory_ledger_mut(&mut player_runtime)
        .begin_world_drop(slot, Some(1))
        .unwrap();
    let predicted = runtime
        .inventory_ledger(&player_runtime)
        .displayed_stack(slot)
        .unwrap();
    assert_eq!(predicted.count, 63);
    assert_eq!(predicted.stack_network_id, request);
}
