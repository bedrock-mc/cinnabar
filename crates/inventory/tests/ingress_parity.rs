use inventory::inventory_ledger::{
    PERSONAL_INVENTORY_WINDOW_TYPE, PLAYER_INVENTORY_SLOT_COUNT, SMALL_STORAGE_SLOT_COUNT,
};
use inventory::{InventorySession, PlayerInventoryLedger, PlayerInventorySlot};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, InventorySlotEvent, InventoryTransactionEvent, ItemStackResponseEvent,
    NetworkItemStack, SlotIdentity, StackResponse, StackResponseStatus,
};

/// Builds a plain original stack with a positive server identity.
fn item(id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        count,
        stack_network_id: id,
        ..NetworkItemStack::empty()
    }
}

/// Makes one content event with no storage-item payload.
fn content(container: ContainerIdentity, slots: Vec<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    })
}

/// Addresses one wire slot without a storage-item payload.
fn update(container: ContainerIdentity, slot: u16, stack: NetworkItemStack) -> InventorySlotEvent {
    InventorySlotEvent {
        identity: SlotIdentity { container, slot },
        stack,
        storage_item: None,
    }
}

/// Creates an empty, fully observed player inventory with the selected authority.
fn ready(authority: InventoryAuthority) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(authority));
    ledger.apply(&content(
        ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT],
    ));
    ledger
}

/// Names a container on an ordinary window, retaining an arbitrary dynamic id.
fn named(window: i32, name: u8, dynamic: Option<u32>) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(window),
        slot_type: Some(name),
        dynamic_id: dynamic,
    }
}

/// Opens one ordinary chest and observes all its cells.
fn chest() -> PlayerInventoryLedger {
    let mut ledger = ready(InventoryAuthority::Server);
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: inventory::inventory_ledger::GENERIC_STORAGE_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let mut slots = vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT];
    slots[2] = item(80, 3);
    ledger.apply(&content(
        named(7, protocol::CONTAINER_NAME_LEVEL_ENTITY, Some(91)),
        slots,
    ));
    ledger
}

/// Rejects one request without adding unrelated corrections.
fn reject(ledger: &mut PlayerInventoryLedger, request_id: i32) {
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: vec![StackResponse {
            status: StackResponseStatus::Rejected,
            request_id,
            containers: vec![].into(),
        }]
        .into(),
    }));
}

/// A full-inventory grant restatement with no cursor must not itself enter recovery.
#[test]
fn full_grant_without_cursor_preserves_world_drop_admission() {
    for authority in [InventoryAuthority::Client, InventoryAuthority::Server] {
        for form in 0..3 {
            let mut ledger = ready(authority);
            let slots = (0..PLAYER_INVENTORY_SLOT_COUNT)
                .map(|n| item(100 + n as i32, 64))
                .collect::<Vec<_>>();
            ledger.apply(&content(
                ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                slots.clone(),
            ));
            let event = match form {
                0 => content(
                    ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                    slots,
                ),
                1 => InventoryEvent::Slot(update(
                    ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                    0,
                    item(100, 64),
                )),
                _ => InventoryEvent::Transaction(InventoryTransactionEvent {
                    slots: vec![update(
                        ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                        0,
                        item(100, 64),
                    )]
                    .into(),
                    skipped_actions: 0,
                }),
            };
            ledger.apply(&event);
            assert!(ledger.cursor_stack().is_none());
            assert!(!ledger.resync_required());
            assert!(ledger.begin_world_drop(0, Some(1)).is_ok());
        }
    }
}

/// A transaction is visible as one FIFO event and cannot mutate a captured snapshot.
#[test]
fn transaction_fifo_publishes_all_cells_together() {
    let mut session = InventorySession::new(1);
    session.publish_inventory_authority(InventoryAuthority::Server);
    session.ledger_mut().apply(&content(
        ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT],
    ));
    let before = session.ledger_snapshot();
    let event = InventoryEvent::Transaction(InventoryTransactionEvent {
        slots: vec![
            update(
                ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                0,
                item(100, 4),
            ),
            update(
                ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                1,
                item(101, 5),
            ),
        ]
        .into(),
        skipped_actions: 0,
    });
    session.enqueue_inventory_event(1, 1, event).unwrap();
    assert!(session.ledger().displayed_stack(0).is_none());
    assert!(session.ledger().displayed_stack(1).is_none());
    assert!(session.apply_next().is_some());
    assert_eq!(session.ledger().displayed_stack(0).unwrap().count, 4);
    assert_eq!(session.ledger().displayed_stack(1).unwrap().count, 5);
    assert!(before.displayed_stack(0).is_none());
    assert!(before.displayed_stack(1).is_none());
    session.begin_session(2);
    assert_eq!(session.inventory_authority(), None);
    assert_eq!(
        session.ledger().slot_state(0),
        Some(PlayerInventorySlot::Unknown)
    );
    assert_eq!(session.pending_inventory_len(), 0);
    assert!(
        session
            .enqueue_inventory_event(1, 2, InventoryEvent::Authority(InventoryAuthority::Server))
            .is_err()
    );
}

/// A grant to backing during an active take must survive rejection of that take.
#[test]
fn grant_during_prediction_survives_rejection_for_every_ingress_form() {
    for form in 0..3 {
        let mut ledger = ready(InventoryAuthority::Server);
        ledger.apply(&InventoryEvent::Slot(update(
            ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
            0,
            item(100, 8),
        )));
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
        let request = ledger.begin_take_count(0, 2).unwrap();
        assert!(ledger.mark_transport_enqueued(1));
        let grant = item(200, 16);
        let event = match form {
            0 => {
                let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
                slots[0] = grant.clone();
                content(
                    ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                    slots,
                )
            }
            1 => InventoryEvent::Slot(update(
                ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                0,
                grant.clone(),
            )),
            _ => InventoryEvent::Transaction(InventoryTransactionEvent {
                slots: vec![update(
                    ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                    0,
                    grant.clone(),
                )]
                .into(),
                skipped_actions: 0,
            }),
        };
        ledger.apply(&event);
        assert_eq!(ledger.displayed_stack(0).unwrap().count, 6);
        reject(&mut ledger, request);
        assert_eq!(ledger.displayed_stack(0), Some(&grant));
        assert!(ledger.cursor_stack().is_none());
        assert!(!ledger.resync_required());
    }
}

/// Content on an ordinary open window must ignore a changed dynamic descriptor.
#[test]
fn ordinary_chest_content_uses_window_when_dynamic_id_changes() {
    let mut ledger = chest();
    let mut slots = vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT];
    slots[2] = item(200, 9);
    ledger.apply(&content(
        named(7, protocol::CONTAINER_NAME_LEVEL_ENTITY, Some(92)),
        slots,
    ));
    assert_eq!(ledger.storage_stack(2), Some(&item(200, 9)));
}

/// A slot on the open chest must remain a chest slot even with an unfamiliar name.
#[test]
fn ordinary_chest_slot_uses_window_when_name_is_unfamiliar() {
    let mut ledger = chest();
    ledger.apply(&InventoryEvent::Slot(update(
        named(7, 211, None),
        2,
        item(200, 9),
    )));
    assert_eq!(ledger.storage_stack(2), Some(&item(200, 9)));
}

/// A foreign ordinary window cannot write the cursor by claiming its container name.
#[test]
fn foreign_window_cursor_name_does_not_change_cursor() {
    let mut ledger = ready(InventoryAuthority::Server);
    ledger.apply(&InventoryEvent::Slot(update(
        named(6, protocol::CONTAINER_NAME_CURSOR, None),
        0,
        item(200, 9),
    )));
    assert!(ledger.cursor_stack().is_none());
}

/// Short player Content replaces the native container rather than retaining its old tail.
#[test]
fn short_player_content_does_not_preserve_tail_stack() {
    let mut ledger = ready(InventoryAuthority::Server);
    ledger.apply(&InventoryEvent::Slot(update(
        ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        35,
        item(100, 8),
    )));
    ledger.apply(&content(
        ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        vec![NetworkItemStack::empty()],
    ));
    assert!(ledger.displayed_stack(35).is_none());
}

/// The offhand Content handler uses the first descriptor when the vector is nonempty.
#[test]
fn offhand_content_uses_first_descriptor_of_nonempty_vector() {
    let mut ledger = ready(InventoryAuthority::Server);
    ledger.apply(&content(
        ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
        vec![item(100, 1), item(101, 1)],
    ));
    assert_eq!(
        ledger.target_stack(inventory::inventory_ledger::InventoryTarget::Offhand),
        Some(&item(100, 1))
    );
}
