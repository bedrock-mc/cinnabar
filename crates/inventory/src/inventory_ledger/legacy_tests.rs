use std::sync::Arc;

use bytes::BytesMut;
use protocol::wire::valentine::bedrock::{
    codec::BedrockCodec,
    version::v1_26_51::{
        EnumsInventorySourceType, InventoryAction, InventoryTransactionPacket,
        InventoryTransactionPacketTransaction, McpePacketData,
    },
};
use protocol::{
    ContainerCloseEvent, ContainerIdentity, ContainerOpenEvent, InventoryContentEvent,
    InventoryEvent, InventorySlotEvent, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion,
    SlotIdentity,
};
use sha2::{Digest, Sha256};

use super::*;

fn stack(id: i32, count: u16) -> NetworkItemStack {
    let extra_data: Arc<[u8]> = Arc::from([0; 10]);
    NetworkItemStack {
        network_id: id,
        count,
        stack_network_id: -1,
        metadata: 3,
        block_runtime_id: 11,
        nbt_digest: Sha256::digest(&extra_data).into(),
        extra_data,
    }
}

fn ready() -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = stack(6, 12);
    slots[2] = stack(7, 1);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: [
            ItemRegistryEntry {
                identifier: Arc::from("minecraft:apple"),
                network_id: 6,
                component_based: false,
                version: ItemRegistryVersion::None,
                component_digest: [0; 32],
                negotiated_max_stack_size: None,
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            },
            ItemRegistryEntry {
                identifier: Arc::from("minecraft:stick"),
                network_id: 7,
                component_based: false,
                version: ItemRegistryVersion::None,
                component_digest: [0; 32],
                negotiated_max_stack_size: None,
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            },
        ]
        .into(),
    });
    ledger
}

fn flush(ledger: &mut PlayerInventoryLedger) -> Vec<Packet> {
    let mut packets = Vec::new();
    while let Some((packet, count)) = ledger.pending_batch().unwrap() {
        packets.push(packet);
        for _ in 0..count {
            assert!(ledger.mark_transport_enqueued(10));
        }
        assert!(packets.len() < 20, "admitted commands must leave the queue");
    }
    packets
}

fn open(ledger: &mut PlayerInventoryLedger) {
    assert!(ledger.request_personal_open(42));
    flush(ledger);
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    assert!(ledger.personal_inventory_desired_open());
    assert!(!ledger.poll_timeout(10_000));
}

fn normal(packet: Packet) -> Vec<InventoryAction> {
    let McpePacketData::InventoryTransactionPacket(transaction) = packet.data else {
        panic!("legacy gestures must send normal InventoryTransaction");
    };
    assert_eq!(transaction.legacy_request_id.id, 0);
    assert!(transaction.legacy_set_item_slots.is_none());
    let mut bytes = BytesMut::new();
    transaction.encode(&mut bytes).unwrap();
    let decoded = InventoryTransactionPacket::decode(&mut bytes.freeze(), ()).unwrap();
    assert_eq!(decoded, *transaction);
    let InventoryTransactionPacketTransaction::NormalTransactionData(normal) = decoded.transaction
    else {
        panic!("ordinary inventory mutations require the normal transaction body");
    };
    normal.actions.actions
}

#[test]
fn legacy_personal_inventory_uses_the_open_and_close_handshake() {
    let mut ledger = ready();
    open(&mut ledger);
    ledger.request_personal_close();
    flush(&mut ledger);
    assert!(!ledger.personal_inventory_desired_open());
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert!(ledger.personal.is_none());
    open(&mut ledger);
}

#[test]
fn legacy_split_pipeline_preserves_descriptors_and_commits_without_response() {
    let mut ledger = ready();
    open(&mut ledger);
    ledger.begin_take_count(0, 5).unwrap();
    ledger.begin_place_count(1, 2).unwrap();
    let (first, entries) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(entries, 1);
    let actions = normal(first.clone());
    assert_eq!(ledger.pending_batch().unwrap().unwrap().0, first);
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].source.container_id, Some(0));
    assert_eq!(actions[0].slot, 0);
    assert_eq!(actions[0].from_item.stacksize, 12);
    assert_eq!(actions[0].to_item.stacksize, 7);
    assert_eq!(
        actions[1].source.container_id,
        Some(protocol::UI_INVENTORY_WINDOW_ID as i8)
    );
    assert_eq!(actions[1].slot, 0);
    assert_eq!(actions[1].from_item.id, 0);
    assert_eq!(actions[1].to_item.stacksize, 5);
    assert_eq!(actions[1].to_item.auxvalue, 3);
    assert_eq!(actions[1].to_item.block_runtime_id, 11);
    assert_eq!(actions[1].to_item.user_data_buffer, vec![0; 10]);
    assert!(actions[1].to_item.net_id_variant.is_none());
    assert!(ledger.mark_transport_enqueued(10));
    assert_eq!(ledger.pending_request_count(), 1);
    let actions = normal(ledger.pending_batch().unwrap().unwrap().0);
    assert_eq!(actions[0].from_item.stacksize, 5);
    assert_eq!(actions[0].to_item.stacksize, 3);
    assert_eq!(actions[1].source.container_id, Some(0));
    assert_eq!(actions[1].slot, 1);
    assert_eq!(actions[1].to_item.stacksize, 2);
    flush(&mut ledger);
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 7);
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 2);
    assert_eq!(ledger.cursor_stack().unwrap().count, 3);
    assert!(!ledger.poll_timeout(50_000));
    assert!(!ledger.resync_required());
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
            slot: 1,
        },
        stack: stack(6, 1),
        storage_item: None,
    }));
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 1);
}

#[test]
fn legacy_chest_take_quick_move_swap_and_drop_use_normal_transactions() {
    let mut ledger = ready();
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let mut slots = vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT];
    slots[3] = stack(6, 8);
    slots[4] = stack(7, 2);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(7),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.begin_storage_click(3).unwrap();
    let actions = normal(flush(&mut ledger).remove(0));
    assert_eq!(actions[0].source.container_id, Some(7));
    assert_eq!(actions[0].slot, 3);
    assert_eq!(actions[0].from_item.stacksize, 8);
    assert_eq!(actions[0].to_item.stacksize, 0);
    assert_eq!(ledger.cursor_stack().unwrap().count, 8);
    ledger.begin_storage_click(4).unwrap();
    normal(flush(&mut ledger).remove(0));
    assert_eq!(ledger.storage_stack(4).unwrap().network_id, 6);
    assert_eq!(ledger.cursor_stack().unwrap().network_id, 7);
    ledger.begin_drop(DropSource::Cursor, Some(1)).unwrap();
    let actions = normal(flush(&mut ledger).remove(0));
    let drop = actions
        .iter()
        .find(|action| action.source.source_type == EnumsInventorySourceType::Worldinteraction)
        .expect("a drop needs its balancing world action");
    assert_eq!(drop.from_item.id, 0);
    assert_eq!(drop.to_item.id, 7);
    assert_eq!(drop.to_item.stacksize, 1);
    assert_eq!(drop.source.container_id, None);
    ledger
        .begin_quick_move(InventoryTarget::Storage(4))
        .unwrap();
    let actions = normal(flush(&mut ledger).remove(0));
    assert!(
        actions
            .iter()
            .any(|action| action.source.container_id == Some(0))
    );
    assert!(ledger.storage_stack(4).is_none());
    assert_eq!(ledger.pending_request_count(), 0);
    ledger.request_storage_close();
    let packets = flush(&mut ledger);
    assert_eq!(packets.len(), 2);
    normal(packets[0].clone());
    assert!(matches!(
        packets[1].data,
        McpePacketData::ContainerClosePacket(_)
    ));
    assert!(ledger.cursor_stack().is_none());
    assert!(!ledger.resync_required());
}

#[test]
fn legacy_failed_send_keeps_retry_and_close_returns_before_control_packet() {
    let mut session = crate::InventorySession::new(1);
    *session.ledger_mut() = ready();
    open(session.ledger_mut());
    session.ledger_mut().begin_take_count(0, 5).unwrap();
    let mut attempts = Vec::new();
    assert_eq!(
        session.flush_inventory_send(20, |packet| {
            attempts.push(packet);
            Err("full")
        }),
        Err("full")
    );
    assert_eq!(session.ledger().pending_request_count(), 1);
    assert_eq!(
        session
            .ledger()
            .confirmed
            .get(Cell::Inventory(0))
            .unwrap()
            .stack
            .count,
        12
    );
    session
        .flush_inventory_send(21, |packet| {
            attempts.push(packet);
            Ok::<_, &str>(())
        })
        .unwrap();
    assert_eq!(attempts[0], attempts[1]);
    assert_eq!(
        session
            .ledger()
            .confirmed
            .get(Cell::Inventory(0))
            .unwrap()
            .stack
            .count,
        7
    );
    session.ledger_mut().request_personal_close();
    let packets = flush(session.ledger_mut());
    assert_eq!(packets.len(), 2);
    normal(packets[0].clone());
    assert!(matches!(
        packets[1].data,
        McpePacketData::ContainerClosePacket(_)
    ));
    assert!(session.ledger().cursor_stack().is_none());
    assert_eq!(session.ledger().displayed_stack(0).unwrap().count, 12);
}

#[test]
fn legacy_repeated_authority_does_not_erase_open_storage_or_predictions() {
    let mut ledger = ready();
    open(&mut ledger);
    ledger.begin_take_count(0, 1).unwrap();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    assert!(ledger.personal_inventory_desired_open());
    assert_eq!(ledger.pending_request_count(), 1);
    normal(flush(&mut ledger).remove(0));
    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
}

#[test]
fn legacy_hotbar_swaps_and_fixed_surfaces_keep_wire_item_ids() {
    let mut ledger = ready();
    open(&mut ledger);
    let mut original = stack(7, 1);
    original.stack_network_id = 91;
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
            slot: 2,
        },
        stack: original.clone(),
        storage_item: None,
    }));
    ledger
        .begin_hotbar_swap(InventoryTarget::Armor(2), 0)
        .unwrap();
    let actions = normal(flush(&mut ledger).remove(0));
    assert_eq!(
        actions[0].source.container_id,
        Some(protocol::ARMOR_WINDOW_ID as i8)
    );
    assert_eq!(actions[0].slot, 2);
    assert_eq!(actions[0].from_item.net_id_variant, Some(91));
    assert_eq!(actions[1].to_item.net_id_variant, Some(91));
    ledger.begin_click(0).unwrap();
    normal(flush(&mut ledger).remove(0));
    ledger
        .begin_target_gesture(InventoryTarget::Offhand, CellGesture::Click)
        .unwrap();
    let actions = normal(flush(&mut ledger).remove(0));
    assert_eq!(
        actions[1].source.container_id,
        Some(protocol::OFFHAND_WINDOW_ID as i8)
    );
    assert_eq!(actions[1].slot, 0);
    assert_eq!(actions[1].to_item.net_id_variant, Some(91));
    assert_eq!(
        ledger.target_stack(InventoryTarget::Offhand),
        Some(&original)
    );
}

#[test]
fn legacy_authoritative_restatement_before_retry_retires_stale_unsent_history() {
    let mut ledger = ready();
    open(&mut ledger);
    ledger.begin_take_count(0, 5).unwrap();
    ledger.begin_place_count(1, 2).unwrap();
    ledger.note_transport_pressure(20);
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
            slot: 0,
        },
        stack: stack(6, 1),
        storage_item: None,
    }));
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 1);
    assert!(ledger.displayed_stack(1).is_none());
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(ledger.pending_batch().unwrap().is_none());
    assert!(!ledger.resync_required());
}

#[test]
fn legacy_later_slot_correction_preserves_the_surviving_cursor_owner() {
    let mut ledger = ready();
    open(&mut ledger);
    ledger.begin_take_count(0, 5).unwrap();
    ledger.begin_place_count(1, 2).unwrap();
    ledger.note_transport_pressure(20);
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
            slot: 1,
        },
        stack: stack(7, 1),
        storage_item: None,
    }));
    assert_eq!(ledger.pending_request_count(), 1);
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 7);
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 1);
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(5));
    let actions = normal(flush(&mut ledger).remove(0));
    assert_eq!(actions[1].to_item.stacksize, 5);
    assert_eq!(ledger.cursor_stack().unwrap().count, 5);
    assert!(!ledger.resync_required());
}
