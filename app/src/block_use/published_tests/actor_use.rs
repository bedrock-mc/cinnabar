use super::*;
use protocol::wire::valentine::bedrock::version::v1_26_51::{
    EnumsItemUseOnActorInventoryTransactionActionType as ActorAction,
    InventoryTransactionPacketTransaction, McpePacketData,
};

/// Adds a selectable actor after the fixture's latest terrain update.
pub(super) fn spawn(world: &mut World, z: f32) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(
            sequence,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 99,
                runtime_id: 99,
                kind: protocol::ActorKind::Entity {
                    identifier: "minecraft:villager_v2".into(),
                },
                position: [4.5, 1.5, z],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: protocol::NetworkItemStack::empty(),
                metadata: [].into(),
                attributes: [].into(),
                properties: [].into(),
                links: [].into(),
            })),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while stream.committed_sequence() < sequence {
        stream.poll([4.5, 2.620_01, 8.5], 0);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
fn actor_use_is_admitted_on_the_press_frame_before_the_next_physics_tick() {
    let (mut world, mut captured) = fixture();
    spawn(&mut world, 7.5);
    let tick = world.resource::<MovementTicker>().completed_tick();
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(world.resource::<MovementTicker>().completed_tick(), tick);
    let packets = captured.drain();
    assert_eq!(packets.len(), 1);
    let McpePacketData::InventoryTransactionPacket(packet) = &packets[0].data else {
        panic!("the press must emit an actor transaction");
    };
    let InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(tx) =
        &packet.transaction
    else {
        panic!("the nearer actor must consume the block and air use");
    };
    assert_eq!(tx.action_type, ActorAction::Interact);
    assert_eq!(tx.runtime_id.actor_runtime_id, 99);
    assert_eq!(tx.slot, 0);
    assert_eq!(
        [tx.from_position.x, tx.from_position.y, tx.from_position.z],
        [4.5, 2.620_01, 8.5]
    );
    assert!(world.resource::<BlockUseRuntime>().press_interacted());
    hold_use(&mut world);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        captured.drain().is_empty(),
        "holding must not duplicate an actor press"
    );
}

#[test]
fn a_block_occludes_actor_use() {
    let (mut world, mut captured) = fixture();
    spawn(&mut world, 5.5);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]]);
}

#[test]
fn a_refused_actor_press_remains_pending_without_consuming_air_use() {
    let (mut world, _) = fixture();
    spawn(&mut world, 7.5);
    let (network, _guard) = NetworkHandle::with_command_capacity(1);
    network
        .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
        .unwrap();
    world.insert_resource(network);
    world.run_system_cached(produce_block_use).unwrap();
    let runtime = world.resource::<BlockUseRuntime>();
    assert!(!runtime.press_interacted());
    assert!(
        runtime
            .due(
                true,
                102,
                RepeatClock {
                    now_millis: 1_000,
                    sneaking: false,
                    speed: 0.0,
                    survival: true
                }
            )
            .is_some()
    );
    let (network, mut captured) = NetworkHandle::stub_capturing_packets();
    world.insert_resource(network);
    hold_use(&mut world);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(captured.drain().len(), 1);
    assert!(world.resource::<BlockUseRuntime>().press_interacted());
}
