//! Selected-slot equipment authority and presentation.

use super::*;

#[test]
fn the_selected_slot_presents_the_equipment_echo_before_inventory_content() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.retain_local_selected_equipment(
        &mut player_runtime,
        1,
        EquipmentEvent {
            actor_runtime_id: 7,
            stack: stack(41),
            inventory_slot: 2,
            selected_slot: 2,
            window_id: 0,
            handedness: Some(ActorHandedness::Right),
        },
    );
    // No inventory content has arrived: the MobEquipment echo is the
    // authoritative selected stack, and only for its own slot.
    assert_eq!(
        player_runtime
            .presented_hotbar_stack(2)
            .map(|stack| stack.network_id),
        Some(41)
    );
    assert_eq!(player_runtime.presented_hotbar_stack(1), None);
}

#[test]
fn known_selected_ledger_state_overrides_the_equipment_bootstrap() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    player_runtime.inventory.set_local_selected_slot(2);
    runtime.retain_local_selected_equipment(
        &mut player_runtime,
        1,
        EquipmentEvent {
            actor_runtime_id: 7,
            stack: stack(41),
            inventory_slot: 2,
            selected_slot: 2,
            window_id: 0,
            handedness: Some(ActorHandedness::Right),
        },
    );
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: inventory_container(0),
                slot: 2,
            },
            stack: NetworkItemStack::empty(),
            storage_item: None,
        }));

    let empty_snapshot = player_runtime.selected_stack_snapshot().unwrap();
    assert_eq!(empty_snapshot.slot, 2);
    assert_eq!(
        empty_snapshot.state,
        inventory::inventory_ledger::PlayerInventorySlot::Empty
    );
    assert_eq!(player_runtime.selected_stack(), None);
    assert_eq!(player_runtime.presented_hotbar_stack(2), None);

    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: inventory_container(0),
                slot: 2,
            },
            stack: stack(77),
            storage_item: None,
        }));

    assert_eq!(
        player_runtime.selected_stack().map(|item| item.network_id),
        Some(77)
    );
    assert_eq!(
        player_runtime
            .presented_hotbar_stack(2)
            .map(|item| item.network_id),
        Some(77)
    );
}

#[test]
fn server_equipment_selects_its_matching_hotbar_slot() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    player_runtime.inventory.set_local_selected_slot(2);
    runtime.retain_local_selected_equipment(
        &mut player_runtime,
        1,
        EquipmentEvent {
            actor_runtime_id: 7,
            stack: stack(41),
            inventory_slot: 3,
            selected_slot: 3,
            window_id: 0,
            handedness: Some(ActorHandedness::Right),
        },
    );
    assert_eq!(player_runtime.selected_hotbar_slot(), Some(3));
    assert_eq!(
        player_runtime.selected_stack().map(|item| item.network_id),
        Some(41)
    );

    let mut player_runtime = player_state::PlayerState::new(1);
    let mut equipment_only = UiRuntime::new(1);
    equipment_only.retain_local_selected_equipment(
        &mut player_runtime,
        1,
        EquipmentEvent {
            actor_runtime_id: 7,
            stack: stack(41),
            inventory_slot: 250,
            selected_slot: 250,
            window_id: 0,
            handedness: Some(ActorHandedness::Right),
        },
    );
    assert_eq!(player_runtime.selected_hotbar_slot(), None);
    assert_eq!(player_runtime.selected_stack(), None);
}

#[test]
fn non_forcing_server_selection_does_not_override_local_prediction() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    player_runtime.inventory.set_local_selected_slot(2);
    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            1,
            1,
            InventoryEvent::SelectedSlot(SelectedSlotEvent {
                container: inventory_container(0),
                slot: 6,
                select_slot: false,
            }),
        )
        .unwrap();

    runtime.drain_pending_inventory(&mut player_runtime);

    assert_eq!(player_runtime.selected_hotbar_slot(), Some(2));
}
