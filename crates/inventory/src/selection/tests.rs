use super::*;
use protocol::{
    ContainerIdentity, InventoryAuthority, InventoryContentEvent, InventoryEvent,
    InventorySlotEvent,
};

#[test]
fn offhand_equipment_tracks_count_identity_clear_and_session_reset() {
    let mut session = InventorySession::new(1);
    session.publish_local_runtime_id(1, 42).unwrap();
    assert!(
        session
            .changed_offhand_equipment_packet()
            .unwrap()
            .is_none()
    );
    let mut stack = protocol::NetworkItemStack {
        network_id: 7,
        stack_network_id: 13,
        count: 4,
        ..protocol::NetworkItemStack::empty()
    };
    for count in [4, 3, 0] {
        stack.count = count;
        replace_offhand_stack(&mut session, stack.clone());
        let packet = session.changed_offhand_equipment_packet().unwrap().unwrap();
        assert_eq!(
            protocol::test_support::packets::equipment(&packet)
                .unwrap()
                .count,
            count
        );
        assert_eq!(
            protocol::encode(&packet).unwrap(),
            protocol::encode(&session.changed_offhand_equipment_packet().unwrap().unwrap())
                .unwrap()
        );
        session.acknowledge_offhand_equipment();
        assert!(
            session
                .changed_offhand_equipment_packet()
                .unwrap()
                .is_none()
        );
        if count != 0 {
            stack.stack_network_id = -3;
            replace_offhand_stack(&mut session, stack.clone());
            assert!(
                session
                    .changed_offhand_equipment_packet()
                    .unwrap()
                    .is_none()
            );
        }
    }
    stack.count = 1;
    replace_offhand_stack(&mut session, stack.clone());
    session.acknowledge_offhand_equipment();
    stack.metadata = 3;
    replace_offhand_stack(&mut session, stack.clone());
    assert!(
        session
            .changed_offhand_equipment_packet()
            .unwrap()
            .is_some()
    );
    session.acknowledge_offhand_equipment();
    session.begin_session(2);
    session.publish_local_runtime_id(2, 99).unwrap();
    assert!(
        session
            .changed_offhand_equipment_packet()
            .unwrap()
            .is_none()
    );
    replace_offhand_stack(&mut session, stack);
    assert!(
        session
            .changed_offhand_equipment_packet()
            .unwrap()
            .is_some()
    );
}

#[test]
fn ignored_offhand_slot_does_not_change_equipment_announcements() {
    let mut session = InventorySession::new(1);
    session.publish_local_runtime_id(1, 42).unwrap();
    replace_offhand_stack(
        &mut session,
        protocol::NetworkItemStack {
            network_id: 7,
            stack_network_id: 13,
            count: 4,
            ..protocol::NetworkItemStack::empty()
        },
    );
    let pending =
        protocol::encode(&session.changed_offhand_equipment_packet().unwrap().unwrap()).unwrap();
    for acknowledged in [false, true] {
        if acknowledged {
            session.acknowledge_offhand_equipment();
        }
        session
            .ledger_mut()
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
                    slot: 0,
                },
                stack: protocol::NetworkItemStack::empty(),
                storage_item: None,
            }));
        let packet = session.changed_offhand_equipment_packet().unwrap();
        if acknowledged {
            assert!(packet.is_none());
        } else {
            assert_eq!(protocol::encode(&packet.unwrap()).unwrap(), pending);
        }
    }
}

/// Replaces known offhand authority through its content receipt.
fn replace_offhand_stack(session: &mut InventorySession, stack: protocol::NetworkItemStack) {
    session
        .ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
            slots: Arc::from([stack]),
            storage_item: protocol::NetworkItemStack::empty(),
        }));
}

/// Replaces one known hotbar cell without changing the selected slot.
fn replace_held_stack(session: &mut InventorySession, stack: protocol::NetworkItemStack) {
    session
        .ledger_mut()
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 2,
            },
            stack,
            storage_item: None,
        }));
}

/// Builds original item data with the same fields in either compound order.
fn held_compound(reverse: bool, damage: i32) -> std::sync::Arc<[u8]> {
    let mut extra = vec![0xff, 0xff, 1, 10, 0, 0];
    let mut fields = [("Damage", damage), ("RepairCost", 3)];
    if reverse {
        fields.reverse();
    }
    for (name, value) in fields {
        extra.push(3);
        extra.extend_from_slice(&(name.len() as u16).to_le_bytes());
        extra.extend_from_slice(name.as_bytes());
        extra.extend_from_slice(&value.to_le_bytes());
    }
    extra.push(0);
    extra.extend_from_slice(&[0; 8]);
    extra.into()
}

#[test]
fn held_equipment_ignores_compound_order_but_keeps_value_changes() {
    use sha2::{Digest, Sha256};
    let mut session = InventorySession::new(1);
    session.publish_local_runtime_id(1, 42).unwrap();
    let mode = Some(PlayerGameMode::Survival);
    let mut stack = protocol::NetworkItemStack {
        network_id: 7,
        count: 1,
        extra_data: held_compound(false, 2),
        ..protocol::NetworkItemStack::empty()
    };
    stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
    replace_held_stack(&mut session, stack.clone());
    session.queue_local_hotbar_selection(2, mode);
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_some()
    );
    assert!(session.clear_pending_hotbar_selection(2));
    stack.extra_data = held_compound(true, 2);
    stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
    replace_held_stack(&mut session, stack.clone());
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_none()
    );
    stack.extra_data = held_compound(true, 4);
    stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
    replace_held_stack(&mut session, stack);
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_some()
    );
}

#[test]
fn held_equipment_resends_same_slot_count_changes_and_depletion() {
    let mut session = InventorySession::new(1);
    session.publish_local_runtime_id(1, 42).unwrap();
    let mode = Some(PlayerGameMode::Survival);
    let mut stack = protocol::NetworkItemStack {
        network_id: 7,
        stack_network_id: 13,
        count: 4,
        ..protocol::NetworkItemStack::empty()
    };
    replace_held_stack(&mut session, stack.clone());
    session.queue_local_hotbar_selection(2, mode);
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_some()
    );
    assert!(session.clear_pending_hotbar_selection(2));
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_none()
    );

    stack.stack_network_id = -3;
    replace_held_stack(&mut session, stack.clone());
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_none()
    );

    stack.count = 3;
    replace_held_stack(&mut session, stack.clone());
    let (slot, packet) = session
        .changed_held_equipment_packet(mode)
        .unwrap()
        .expect("count change must update held equipment");
    assert_eq!(slot, 2);
    assert_eq!(
        protocol::test_support::packets::equipment(&packet)
            .unwrap()
            .count,
        3
    );
    let (_, retry) = session
        .changed_held_equipment_packet(mode)
        .unwrap()
        .unwrap();
    assert_eq!(
        protocol::encode(&packet).unwrap(),
        protocol::encode(&retry).unwrap()
    );
    assert!(session.clear_pending_hotbar_selection(slot));
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_none()
    );

    stack.metadata = 5;
    replace_held_stack(&mut session, stack);
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_some()
    );
    assert!(session.clear_pending_hotbar_selection(2));
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_none()
    );

    replace_held_stack(&mut session, protocol::NetworkItemStack::empty());
    let (slot, packet) = session
        .changed_held_equipment_packet(mode)
        .unwrap()
        .expect("depletion must clear held equipment");
    assert_eq!(
        protocol::test_support::packets::equipment(&packet)
            .unwrap()
            .count,
        0
    );
    assert!(session.clear_pending_hotbar_selection(slot));
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_none()
    );

    session.begin_session(2);
    session.publish_local_runtime_id(2, 99).unwrap();
    session.set_local_selected_slot(2);
    replace_held_stack(&mut session, protocol::NetworkItemStack::empty());
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_some()
    );
}

/// Unreported cells announce an empty hand without manufacturing slot authority.
#[test]
fn unknown_hotbar_cell_can_announce_empty_equipment() {
    let mode = Some(PlayerGameMode::Survival);
    let mut session = InventorySession::new(1);
    session.publish_local_runtime_id(1, 42).unwrap();
    let proposal = session.prepare_hotbar_selection(2).unwrap().unwrap();
    assert_eq!(proposal.slot(), 2);
    assert!(session.hotbar_selection_current(&proposal));
    session.queue_local_hotbar_selection(2, mode);
    let (slot, packet) = session.pending_hotbar_packet(mode).unwrap().unwrap();
    assert_eq!(slot, 2);
    assert_eq!(proposal.packet(), &packet);
    let equipment = protocol::test_support::packets::equipment(&packet).unwrap();
    assert_eq!(equipment.network_id, 0);
    assert_eq!(equipment.count, 0);
    assert_eq!(
        session.ledger().backing_slot_state(2),
        Some(PlayerInventorySlot::Unknown)
    );
    assert!(session.clear_pending_hotbar_selection(slot));
    assert!(
        session
            .changed_held_equipment_packet(mode)
            .unwrap()
            .is_none()
    );
}

#[test]
fn server_mob_equipment_forces_the_local_hotbar_slot() {
    let mode = Some(PlayerGameMode::Survival);
    let mut session = InventorySession::new(1);
    session.set_local_selected_slot(2);
    session.queue_local_hotbar_selection(5, mode);

    session.retain_local_selected_equipment(
        7,
        EquipmentEvent {
            actor_runtime_id: 42,
            stack: protocol::NetworkItemStack::empty(),
            inventory_slot: 7,
            selected_slot: 7,
            window_id: protocol::PLAYER_INVENTORY_WINDOW_ID as u8,
            handedness: Some(protocol::ActorHandedness::Right),
        },
    );

    assert_eq!(session.selected_hotbar_slot(mode), Some(7));
    assert_eq!(session.pending_hotbar_selection(), None);
}

#[test]
fn offhand_or_sentinel_mob_equipment_does_not_force_the_local_hotbar_slot() {
    let mode = Some(PlayerGameMode::Survival);
    let mut session = InventorySession::new(1);
    session.set_local_selected_slot(2);

    session.retain_local_selected_equipment(
        7,
        EquipmentEvent {
            actor_runtime_id: 42,
            stack: protocol::NetworkItemStack::empty(),
            inventory_slot: 6,
            selected_slot: 6,
            window_id: protocol::OFFHAND_WINDOW_ID as u8,
            handedness: Some(protocol::ActorHandedness::Left),
        },
    );
    assert_eq!(session.selected_hotbar_slot(mode), Some(2));

    session.retain_local_selected_equipment(
        8,
        EquipmentEvent {
            actor_runtime_id: 42,
            stack: protocol::NetworkItemStack::empty(),
            inventory_slot: 255,
            selected_slot: u8::MAX,
            window_id: protocol::PLAYER_INVENTORY_WINDOW_ID as u8,
            handedness: Some(protocol::ActorHandedness::Right),
        },
    );
    assert_eq!(session.selected_hotbar_slot(mode), Some(2));
}

/// A local drop publishes backing only after its transaction enters transport.
#[test]
fn backing_hotbar_descriptor_waits_for_a_local_backing_mutation() {
    let game_mode = Some(PlayerGameMode::Survival);
    let mut session = InventorySession::new(1);
    session.publish_local_runtime_id(1, 42).unwrap();
    let ledger = session.ledger_mut();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 2,
        },
        stack: protocol::NetworkItemStack {
            network_id: 7,
            stack_network_id: 13,
            count: 4,
            ..protocol::NetworkItemStack::empty()
        },
        storage_item: None,
    }));
    let proposal = session.prepare_hotbar_selection(2).unwrap().unwrap();
    let ledger = session.ledger_mut();
    ledger.begin_world_drop(2, Some(1)).unwrap();
    let PlayerInventorySlot::Present(predicted) = session.ledger().slot_state(2).unwrap() else {
        panic!("the drop leaves three items predicted in the slot");
    };
    assert_eq!(predicted.stack_network_id, -3);
    assert!(session.prepare_hotbar_selection(2).unwrap().is_none());
    assert!(!session.hotbar_selection_current(&proposal));

    session.queue_local_hotbar_selection(2, game_mode);
    assert!(session.pending_hotbar_packet(game_mode).unwrap().is_none());
    assert!(session.ledger_mut().mark_transport_enqueued(0));
    let proposal = session.prepare_hotbar_selection(2).unwrap().unwrap();
    assert!(session.hotbar_selection_current(&proposal));
    let (slot, packet) = session.pending_hotbar_packet(game_mode).unwrap().unwrap();
    assert_eq!(slot, 2);
    assert_eq!(proposal.packet(), &packet);
    let packet = protocol::test_support::packets::equipment(&packet)
        .expect("hotbar selection must build a MobEquipment packet");
    assert_eq!(packet.count, 3);
    assert_eq!(packet.stack_network_id, None);
}

/// Client-owned gestures publish their changed count only after transport accepts them.
#[test]
fn ordered_hotbar_descriptor_waits_for_client_backing_transport() {
    let mut session = InventorySession::new(1);
    session.publish_local_runtime_id(1, 42).unwrap();
    session
        .ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    replace_held_stack(
        &mut session,
        protocol::NetworkItemStack {
            network_id: 7,
            stack_network_id: 13,
            count: 4,
            ..protocol::NetworkItemStack::empty()
        },
    );
    let proposal = session.prepare_hotbar_selection(2).unwrap().unwrap();
    let ledger = session.ledger_mut();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.begin_take_count(2, 1).unwrap();
    assert!(session.prepare_hotbar_selection(2).unwrap().is_none());
    assert!(!session.hotbar_selection_current(&proposal));
    assert!(session.prepare_hotbar_selection(1).unwrap().is_some());

    assert!(session.ledger_mut().mark_transport_enqueued(0));
    let proposal = session.prepare_hotbar_selection(2).unwrap().unwrap();
    assert!(session.hotbar_selection_current(&proposal));
    let sent = protocol::test_support::packets::equipment(proposal.packet()).unwrap();
    assert_eq!(sent.network_id, 7);
    assert_eq!(sent.count, 3);
    assert_eq!(sent.stack_network_id, None);
}

#[test]
fn backing_hotbar_descriptor_ignores_screen_only_item_changes() {
    for backing_present in [false, true] {
        let mode = Some(PlayerGameMode::Survival);
        let mut session = InventorySession::new(1);
        session.publish_local_runtime_id(1, 42).unwrap();
        let ledger = session.ledger_mut();
        ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
        let item = protocol::NetworkItemStack {
            network_id: 7,
            stack_network_id: 13,
            count: 4,
            ..protocol::NetworkItemStack::empty()
        };
        for (window, slot, stack) in [
            (
                protocol::PLAYER_INVENTORY_WINDOW_ID,
                2,
                if backing_present {
                    item.clone()
                } else {
                    protocol::NetworkItemStack::empty()
                },
            ),
            (
                protocol::UI_INVENTORY_WINDOW_ID,
                0,
                if backing_present {
                    protocol::NetworkItemStack::empty()
                } else {
                    item
                },
            ),
        ] {
            ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: ContainerIdentity::window(window),
                    slot,
                },
                stack,
                storage_item: None,
            }));
        }
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(0));
        let gesture = if backing_present {
            crate::inventory_ledger::CellGesture::TakeCount(1)
        } else {
            crate::inventory_ledger::CellGesture::Click
        };
        ledger
            .begin_target_gesture(crate::inventory_ledger::InventoryTarget::Player(2), gesture)
            .unwrap();
        let proposal = session.prepare_hotbar_selection(2).unwrap().unwrap();
        session.queue_local_hotbar_selection(2, mode);
        let (_, packet) = session.pending_hotbar_packet(mode).unwrap().unwrap();
        assert_eq!(proposal.packet(), &packet);
        let sent = protocol::test_support::packets::equipment(&packet).unwrap();
        assert_eq!(sent.network_id, if backing_present { 7 } else { 0 });
        assert_eq!(sent.count, if backing_present { 4 } else { 0 });
        assert!(session.clear_pending_hotbar_selection(2));
        assert!(
            session
                .changed_held_equipment_packet(mode)
                .unwrap()
                .is_none()
        );
    }
}
