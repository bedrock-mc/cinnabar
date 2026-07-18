use bedrock_client::ui_runtime::inventory_router::{
    EquipmentRoute, EquipmentRouteResult, InventoryEquipmentRouter, InventoryRouterError,
    MAX_PRE_IDENTITY_EQUIPMENT,
};
use protocol::{EquipmentEvent, NetworkItemStack};

fn equipment(actor_runtime_id: u64, selected_slot: u8) -> EquipmentEvent {
    EquipmentEvent {
        actor_runtime_id,
        stack: NetworkItemStack::empty(),
        inventory_slot: i32::from(selected_slot),
        selected_slot,
        window_id: 0,
        handedness: None,
    }
}

#[test]
fn pre_identity_equipment_drains_fifo_to_exactly_one_consumer() {
    let mut router = InventoryEquipmentRouter::new(7);
    assert_eq!(
        router.route(7, 10, equipment(42, 1)).unwrap(),
        EquipmentRouteResult::Buffered
    );
    assert_eq!(
        router.route(7, 11, equipment(99, 2)).unwrap(),
        EquipmentRouteResult::Buffered
    );

    let drained = router.publish_local_runtime_id(7, 42).unwrap();
    assert_eq!(drained.len(), 2);
    assert!(matches!(
        &drained[0],
        EquipmentRoute::LocalSelected { fifo_sequence: 10, event }
            if event.actor_runtime_id == 42
    ));
    assert!(matches!(
        &drained[1],
        EquipmentRoute::ActorPresentation { fifo_sequence: 11, event }
            if event.actor_runtime_id == 99
    ));

    assert!(matches!(
        router.route(7, 12, equipment(42, 3)).unwrap(),
        EquipmentRouteResult::Routed(EquipmentRoute::LocalSelected { .. })
    ));
    assert!(matches!(
        router.route(7, 13, equipment(99, 4)).unwrap(),
        EquipmentRouteResult::Routed(EquipmentRoute::ActorPresentation { .. })
    ));
}

#[test]
fn session_replacement_clears_buffer_identity_and_fifo_authority() {
    let mut router = InventoryEquipmentRouter::new(1);
    router.route(1, 100, equipment(42, 0)).unwrap();
    router.begin_session(2);

    assert_eq!(router.session_id(), 2);
    assert_eq!(router.pending_len(), 0);
    assert_eq!(router.local_runtime_id(), None);
    assert_eq!(
        router.route(2, 1, equipment(42, 0)).unwrap(),
        EquipmentRouteResult::Buffered
    );
    assert_eq!(router.publish_local_runtime_id(2, 42).unwrap().len(), 1);
}

#[test]
fn router_rejects_wrong_session_stale_fifo_zero_identity_and_overflow() {
    let mut router = InventoryEquipmentRouter::new(5);
    assert_eq!(
        router.route(4, 1, equipment(42, 0)).unwrap_err(),
        InventoryRouterError::WrongSession {
            expected: 5,
            actual: 4
        }
    );
    assert_eq!(
        router.publish_local_runtime_id(5, 0).unwrap_err(),
        InventoryRouterError::InvalidRuntimeId(0)
    );
    router.route(5, 7, equipment(42, 0)).unwrap();
    assert_eq!(
        router.route(5, 7, equipment(42, 0)).unwrap_err(),
        InventoryRouterError::StaleFifoSequence {
            previous: 7,
            actual: 7
        }
    );

    let mut full = InventoryEquipmentRouter::new(9);
    for fifo in 0..MAX_PRE_IDENTITY_EQUIPMENT as u64 {
        full.route(9, fifo, equipment(42, 0)).unwrap();
    }
    assert_eq!(
        full.route(9, MAX_PRE_IDENTITY_EQUIPMENT as u64, equipment(42, 0))
            .unwrap_err(),
        InventoryRouterError::PreIdentityBufferFull {
            maximum: MAX_PRE_IDENTITY_EQUIPMENT
        }
    );
}
