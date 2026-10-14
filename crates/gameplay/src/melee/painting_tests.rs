use super::*;
use std::sync::Arc;

#[test]
fn native_painting_pick_resolves_unique_id_and_respects_walls_and_removal() {
    let mut world = client_world::WorldAuthority::new(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 999,
            local_player_unique_id: 999,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    let event = protocol::test_support::packets::add_painting_event(
        -73,
        91,
        "backyard",
        2,
        [0.0, 70.5, 3.5],
    )
    .unwrap()
    .unwrap();
    world.apply_ordered_event(event, Some(1)).unwrap();
    for block in [None, Some(4.0)] {
        assert_eq!(
            painting_pick_unique_id(&world, [0.0, 70.5, 0.0], [0.0, 0.0, 1.0], 5.0, block, None),
            Some(-73)
        );
    }
    assert_eq!(
        painting_pick_unique_id(
            &world,
            [0.0, 70.5, 0.0],
            [0.0, 0.0, 1.0],
            5.0,
            Some(2.0),
            None
        ),
        None
    );
    assert_eq!(
        painting_pick_unique_id(&world, [2.0, 70.5, 0.0], [0.0, 0.0, 1.0], 5.0, None, None),
        None
    );
    assert_eq!(
        painting_pick_unique_id(
            &world,
            [0.0, 70.5, 0.0],
            [0.0, 0.0, 1.0],
            5.0,
            None,
            Some(-73)
        ),
        None
    );
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Remove(protocol::ActorRemoveEvent {
                dimension: 0,
                unique_id: -73,
            })),
            Some(2),
        )
        .unwrap();
    assert_eq!(
        painting_pick_unique_id(&world, [0.0, 70.5, 0.0], [0.0, 0.0, 1.0], 5.0, None, None),
        None
    );
}
