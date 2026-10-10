use super::*;
use protocol::wire::valentine::bedrock::version::v1_26_51::{
    EnumsItemUseOnActorInventoryTransactionActionType as ActorAction,
    InventoryTransactionPacketTransaction, McpePacketData,
};

/// Adds a selectable actor after the fixture's latest terrain update.
pub(super) fn spawn(world: &mut World, z: f32) {
    spawn_kind(
        world,
        z,
        protocol::ActorKind::Entity {
            identifier: "minecraft:villager_v2".into(),
        },
    );
}

fn spawn_kind(world: &mut World, z: f32, kind: protocol::ActorKind) {
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
                kind,
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

/// Uses a snowball on a player after the server set `interact_text`, returning the packet kinds.
fn throw_at_player(interact_text: &str) -> Vec<&'static str> {
    let (mut world, mut captured) = fixture();
    super::item_use_tests::hold_snowball(&mut world);
    spawn_kind(
        &mut world,
        7.5,
        protocol::ActorKind::Player {
            uuid: [7; 16],
            username: "target".into(),
        },
    );
    world
        .resource_mut::<UiRuntime>()
        .apply_local_metadata(
            7,
            u64::MAX,
            &[protocol::ActorMetadata {
                key: 100,
                value: protocol::ActorMetadataValue::String(interact_text.into()),
            }],
        )
        .unwrap();
    world.run_system_cached(produce_block_use).unwrap();
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
    captured
        .drain()
        .into_iter()
        .filter_map(|packet| match &packet.data {
            McpePacketData::InventoryTransactionPacket(packet) => match &packet.transaction {
                InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(tx) => {
                    assert_eq!(tx.action_type, ActorAction::Interact);
                    assert_eq!(tx.runtime_id.actor_runtime_id, 99);
                    Some("interact")
                }
                InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(_) => {
                    Some("use")
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// A throwable used on a player with no interaction must still throw, after the interact.
#[test]
fn a_throwable_used_on_a_player_without_an_interaction_still_throws() {
    assert_eq!(throw_at_player(""), ["interact", "use"]);
}

#[test]
fn a_server_offered_player_interaction_consumes_the_use() {
    assert_eq!(throw_at_player("action.interact.ride.horse"), ["interact"]);
}
