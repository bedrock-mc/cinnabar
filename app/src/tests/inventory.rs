use protocol::{EquipmentEvent, NetworkItemStack, WorldEvent};

use crate::{
    runtime::network::{
        EquipmentIngress, publish_equipment_identity, route_equipment_ingress,
        session::SequencedWorldEvent,
    },
    ui_runtime::UiRuntime,
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
            sequence: 10,
            event: WorldEvent::Equipment(equipment(42, 1)),
        },
    )
    .unwrap();
    assert_eq!(buffered, EquipmentIngress::Buffered);

    let drained = publish_equipment_identity(&mut runtime, 7, 42).unwrap();
    assert_eq!(drained, vec![EquipmentIngress::LocalSelected]);
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
                sequence: 1,
                event: WorldEvent::Equipment(equipment(42, 0)),
            },
        )
        .unwrap(),
        EquipmentIngress::LocalSelected
    );

    runtime.begin_session(2);
    assert!(runtime.local_selected_equipment().is_none());
    assert_eq!(
        route_equipment_ingress(
            &mut runtime,
            SequencedWorldEvent {
                sequence: 1,
                event: WorldEvent::Equipment(equipment(42, 0)),
            },
        )
        .unwrap(),
        EquipmentIngress::Buffered
    );
}
