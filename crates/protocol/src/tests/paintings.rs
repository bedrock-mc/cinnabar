use crate::{ActorEvent, ActorKind, WorldEvent, into_world_event};
use bedrock_protocol_1_26_51::{
    packets::AddPainting,
    types::{ActorRuntimeID, ActorUniqueID},
    wire,
};

#[test]
fn add_painting_admits_the_native_spawn_packet() {
    let packet = protocol::Packet::new(AddPainting {
        target_actor_id: ActorUniqueID(-73),
        target_runtime_id: ActorRuntimeID(91),
        position: [711.5, 70.5, 5.03125].into(),
        direction: wire::ZigZag32(2),
        motif: "Kebab".into(),
    });
    let Some(WorldEvent::Actor(ActorEvent::PaintingSpawn { spawn, painting })) =
        into_world_event(packet, 1).expect("well-formed painting packet")
    else {
        panic!("AddPainting must publish a retained actor spawn");
    };
    assert_eq!(spawn.unique_id, -73);
    assert_eq!(spawn.runtime_id, 91);
    assert_eq!(spawn.dimension, 1);
    assert_eq!(spawn.position, [711.5, 70.5, 5.03125]);
    assert!(
        matches!(spawn.kind, ActorKind::Entity {identifier} if identifier.as_ref() == "minecraft:painting")
    );
    assert_eq!(painting.motive.as_ref(), "Kebab");
    assert_eq!(painting.direction, protocol::PaintingDirection::North);
}

#[test]
fn painting_directions_retain_all_walls_and_skip_unknown_values() {
    use protocol::PaintingDirection;
    for (wire, direction, yaw) in [
        (0, PaintingDirection::South, 180.0),
        (1, PaintingDirection::West, 90.0),
        (2, PaintingDirection::North, 0.0),
        (3, PaintingDirection::East, 270.0),
    ] {
        let Some(WorldEvent::Actor(ActorEvent::PaintingSpawn { spawn, painting })) =
            protocol::test_support::packets::add_painting_event(2, 3, "unpacked", wire, [0.0; 3])
                .unwrap()
        else {
            panic!("horizontal painting direction");
        };
        assert_eq!(painting.direction, direction);
        assert_eq!(spawn.yaw, yaw);
        assert_eq!(painting.motive.as_ref(), "unpacked");
    }
    for direction in [-1, 4, i32::MAX] {
        assert!(matches!(
            protocol::test_support::packets::add_painting_event(2, 3, "Kebab", direction, [0.0; 3]),
            Err(protocol::WorldPacketError::Actor(
                protocol::ActorPacketError::InvalidPaintingDirection(_)
            ))
        ));
    }
}

#[test]
fn painting_spawn_rejects_nonfinite_centres_and_bounds_motive_text() {
    assert!(
        protocol::test_support::packets::add_painting_event(2, 3, "Kebab", 0, [f32::NAN, 0.0, 0.0])
            .is_err()
    );
    let oversized = "a".repeat(protocol::MAX_ACTOR_IDENTIFIER_BYTES + 1);
    assert!(
        protocol::test_support::packets::add_painting_event(2, 3, &oversized, 0, [0.0; 3]).is_err()
    );
}

#[test]
fn painting_pick_uses_the_persistent_unique_id_and_inventory_capacity() {
    let packet = protocol::actor_pick_request_packet(-73, false);
    let bedrock_protocol_1_26_51::packets::Packet::ActorPickRequest(pick) = *packet.data else {
        panic!("actor pick request");
    };
    assert_eq!(pick.actor_id.0, -73);
    assert_eq!(pick.max_slots.0, protocol::HOTBAR_SLOT_COUNT);
    assert!(!pick.with_data);
}
