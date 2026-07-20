use protocol::{
    EquipmentEvent, InventoryAuthority, InventoryEvent, NetworkItemStack, WorldBootstrap,
    WorldEvent,
};

use crate::{
    runtime::network::{
        EquipmentIngress, bootstrap_session_generation_is_expected, publish_equipment_identity,
        route_equipment_ingress, route_inventory_ingress, session::SequencedWorldEvent,
    },
    ui_runtime::{MAX_PENDING_INVENTORY_EVENTS, UiRuntime},
};

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
fn runtime_publishes_session_identity_and_routes_fifo_equipment_once() {
    let mut runtime = UiRuntime::new(7);
    let buffered = route_equipment_ingress(
        &mut runtime,
        SequencedWorldEvent {
            session_generation: 7,
            sequence: 10,
            event: WorldEvent::Equipment(equipment(42, 1)),
        },
    )
    .unwrap();
    assert_eq!(buffered, EquipmentIngress::Buffered);

    let drained = publish_equipment_identity(&mut runtime, 7, 42).unwrap();
    assert_eq!(
        drained,
        vec![EquipmentIngress::CommitOnly { fifo_sequence: 10 }]
    );
    assert_eq!(
        runtime
            .local_selected_equipment()
            .expect("local selected equipment")
            .fifo_sequence,
        10
    );

    let remote = route_equipment_ingress(
        &mut runtime,
        SequencedWorldEvent {
            session_generation: 7,
            sequence: 11,
            event: WorldEvent::Equipment(equipment(99, 2)),
        },
    )
    .unwrap();
    let EquipmentIngress::ActorPresentation(remote) = remote else {
        panic!("remote equipment must route only to actor presentation")
    };
    assert_eq!(remote.sequence, 11);
    assert!(matches!(
        remote.event,
        WorldEvent::Equipment(EquipmentEvent {
            actor_runtime_id: 99,
            ..
        })
    ));
}

#[test]
fn session_replacement_clears_published_identity_and_local_selection() {
    let mut runtime = UiRuntime::new(1);
    publish_equipment_identity(&mut runtime, 1, 42).unwrap();
    assert_eq!(
        route_equipment_ingress(
            &mut runtime,
            SequencedWorldEvent {
                session_generation: 1,
                sequence: 1,
                event: WorldEvent::Equipment(equipment(42, 0)),
            },
        )
        .unwrap(),
        EquipmentIngress::CommitOnly { fifo_sequence: 1 }
    );

    runtime.begin_session(2);
    assert!(runtime.local_selected_equipment().is_none());
    assert_eq!(
        route_equipment_ingress(
            &mut runtime,
            SequencedWorldEvent {
                session_generation: 2,
                sequence: 1,
                event: WorldEvent::Equipment(equipment(42, 0)),
            },
        )
        .unwrap(),
        EquipmentIngress::Buffered
    );
}

#[test]
fn consumed_local_equipment_commits_its_global_fifo_slot() {
    let mut runtime = UiRuntime::new(7);
    publish_equipment_identity(&mut runtime, 7, 42).unwrap();
    let ingress = route_equipment_ingress(
        &mut runtime,
        SequencedWorldEvent {
            session_generation: 7,
            sequence: 1,
            event: WorldEvent::Equipment(equipment(42, 0)),
        },
    )
    .unwrap();
    let EquipmentIngress::CommitOnly { fifo_sequence } = ingress else {
        panic!("local equipment must produce a FIFO commit marker")
    };

    let mut stream = client_world::WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream.commit(fifo_sequence).unwrap();
    stream
        .submit(2, WorldEvent::ChunkRadiusUpdated(16))
        .unwrap();

    assert_eq!(stream.stats().admitted_world_events, 0);
    assert_eq!(
        runtime
            .local_selected_equipment()
            .expect("local equipment retained exactly once")
            .fifo_sequence,
        1
    );
}

#[test]
fn inventory_handoff_is_bounded_session_scoped_and_fifo_ordered() {
    let mut runtime = UiRuntime::new(7);
    for sequence in 1..=MAX_PENDING_INVENTORY_EVENTS as u64 {
        runtime
            .enqueue_inventory_event(
                7,
                sequence,
                InventoryEvent::Authority(InventoryAuthority::Server),
            )
            .unwrap();
    }
    assert!(
        runtime
            .enqueue_inventory_event(
                7,
                MAX_PENDING_INVENTORY_EVENTS as u64 + 1,
                InventoryEvent::Authority(InventoryAuthority::Server),
            )
            .is_err()
    );
    assert!(
        runtime
            .enqueue_inventory_event(
                6,
                MAX_PENDING_INVENTORY_EVENTS as u64 + 2,
                InventoryEvent::Authority(InventoryAuthority::Server),
            )
            .is_err()
    );

    let first = runtime
        .pop_inventory_event()
        .expect("oldest inventory event");
    assert_eq!(first.session_generation, 7);
    assert_eq!(first.fifo_sequence, 1);
    assert!(
        runtime
            .enqueue_inventory_event(7, 1, InventoryEvent::Authority(InventoryAuthority::Server),)
            .is_err()
    );

    runtime.begin_session(8);
    assert!(runtime.pop_inventory_event().is_none());
}

#[test]
fn inventory_ingress_is_retained_while_global_fifo_advances() {
    let mut runtime = UiRuntime::new(7);
    let commit_sequence = route_inventory_ingress(
        &mut runtime,
        SequencedWorldEvent {
            session_generation: 7,
            sequence: 1,
            event: WorldEvent::Inventory(InventoryEvent::Authority(InventoryAuthority::Server)),
        },
    )
    .unwrap();
    let mut stream = client_world::WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream.commit(commit_sequence).unwrap();
    stream
        .submit(2, WorldEvent::ChunkRadiusUpdated(16))
        .unwrap();

    assert_eq!(stream.stats().admitted_world_events, 0);
    let retained = runtime.pop_inventory_event().expect("inventory handoff");
    assert_eq!(retained.session_generation, 7);
    assert_eq!(retained.fifo_sequence, 1);
}

#[test]
fn stale_equipment_envelope_is_rejected_instead_of_relabelled() {
    let mut runtime = UiRuntime::new(8);
    publish_equipment_identity(&mut runtime, 8, 42).unwrap();

    let error = route_equipment_ingress(
        &mut runtime,
        SequencedWorldEvent {
            session_generation: 7,
            sequence: 1,
            event: WorldEvent::Equipment(equipment(42, 0)),
        },
    )
    .expect_err("stale network session");

    assert!(matches!(
        error,
        crate::ui_runtime::inventory_router::InventoryRouterError::WrongSession {
            expected: 8,
            actual: 7,
        }
    ));
    assert!(runtime.local_selected_equipment().is_none());
}

#[test]
fn bootstrap_generation_must_match_the_next_ui_and_world_session() {
    assert!(bootstrap_session_generation_is_expected(0, 0, 1));
    assert!(bootstrap_session_generation_is_expected(7, 7, 8));
    assert!(!bootstrap_session_generation_is_expected(7, 7, 7));
    assert!(!bootstrap_session_generation_is_expected(7, 7, 9));
    assert!(!bootstrap_session_generation_is_expected(6, 7, 8));
}
