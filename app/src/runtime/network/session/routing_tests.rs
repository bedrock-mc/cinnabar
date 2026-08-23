use super::*;
use crate::runtime::network::session::{InboundWorldEvent, wrap_inbound_world_event};

#[test]
fn level_chunk_private_ingress_preserves_payload_allocation_and_fifo_sequence() {
    let payload = bytes::Bytes::from(vec![0x5a; 1024 * 1024]);
    let pointer = payload.as_ptr();
    let counter = ReadinessIngressCounter::default();
    let mut sequencer = NetworkSequencer::new(7, 0, 42);

    let ingress = wrap_inbound_world_event(
        &mut sequencer,
        &counter,
        InboundWorldEvent::LevelChunk {
            event: protocol::LevelChunkEvent {
                dimension: 0,
                x: 3,
                z: 4,
                mode: protocol::LevelChunkMode::Inline { count: 0 },
                payload: Vec::new(),
            },
            payload,
        },
    );

    let WorldIngress::LevelChunk {
        session_generation,
        sequence,
        payload,
        ..
    } = ingress
    else {
        panic!("LevelChunk must use the private byte ingress lane")
    };
    assert_eq!(session_generation, 7);
    assert_eq!(sequence, 1);
    assert_eq!(payload.as_ptr(), pointer);
    assert_eq!(counter.pending(), 1);
}

#[test]
fn start_game_inventory_authority_is_fanned_out_as_a_normalized_event() {
    let mut game_data = protocol::GameData {
        start_game: Default::default(),
        item_registry: Default::default(),
        biome_definitions: None,
        entity_identifiers: None,
        creative_content: None,
    };
    assert_eq!(
        start_game_inventory_authority(&game_data),
        InventoryEvent::Authority(InventoryAuthority::Client)
    );
    game_data.start_game.enable_item_stack_net_manager = true;
    assert_eq!(
        start_game_inventory_authority(&game_data),
        InventoryEvent::Authority(InventoryAuthority::Server)
    );
}

#[test]
fn sequence_is_fifo_and_dimension_changes_apply_to_following_packets() {
    let mut sequencer = NetworkSequencer::new(7, 2, 42);
    let first = sequencer.wrap(WorldEvent::ChunkRadiusUpdated(16));
    assert_eq!(first.session_generation, 7);
    assert_eq!(first.sequence, 1);
    assert_eq!(sequencer.current_dimension(), 2);

    let change = sequencer.wrap(WorldEvent::ChangeDimension(ChangeDimensionEvent {
        dimension: 1,
        position: [0.0, 80.0, 0.0],
    }));
    assert_eq!(change.sequence, 2);
    assert_eq!(sequencer.current_dimension(), 1);

    let following = sequencer.wrap(WorldEvent::ChunkRadiusUpdated(8));
    assert_eq!(following.sequence, 3);
}

#[test]
fn foreign_player_movement_is_routed_to_the_actor_stream() {
    let mut sequencer = NetworkSequencer::new(7, 0, 42);
    let movement = |runtime_id| {
        WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id,
            // MovePlayer carries the network-offset position for a standing
            // player whose spawn/render feet position is Y=64.
            position: [1.0, 64.0 + PLAYER_NETWORK_OFFSET, 2.0],
            pitch: 5.0,
            yaw: 90.0,
            head_yaw: 110.0,
            mode: protocol::MovePlayerMode::Teleport,
            on_ground: true,
            teleported: true,
            source_tick: 1_234,
        })
    };

    assert!(matches!(
        sequencer.wrap(movement(42)).event,
        WorldEvent::MovePlayer(MovePlayerEvent { runtime_id: 42, .. })
    ));
    let WorldEvent::Actor(protocol::ActorEvent::Move(remote)) = sequencer.wrap(movement(7)).event
    else {
        panic!("foreign MovePlayer was not routed to the actor stream");
    };
    assert_eq!(remote.runtime_id, 7);
    assert_eq!(remote.dimension, 0);
    assert_eq!(remote.position[0], Some(1.0));
    assert!((remote.position[1].unwrap() - (64.0 + PLAYER_NETWORK_OFFSET)).abs() < 1e-5);
    assert_eq!(remote.position[2], Some(2.0));
    assert_eq!(remote.position_origin, ActorPositionOrigin::NetworkOffset);
    assert_eq!(remote.head_yaw, Some(110.0));
    assert_eq!(remote.on_ground, Some(true));
    assert!(remote.teleported);
    assert_eq!(remote.player_mode, Some(protocol::MovePlayerMode::Teleport));
    assert_eq!(remote.source_tick, Some(1_234));
}

#[test]
fn foreign_move_player_retains_network_origin_for_actor_store_normalization() {
    const SPAWN_FEET_Y: f32 = 64.0;
    let mut sequencer = NetworkSequencer::new(7, 0, 42);
    let movement = WorldEvent::MovePlayer(MovePlayerEvent {
        runtime_id: 7,
        position: [1.0, SPAWN_FEET_Y + PLAYER_NETWORK_OFFSET, 2.0],
        ..Default::default()
    });

    let WorldEvent::Actor(protocol::ActorEvent::Move(remote)) = sequencer.wrap(movement).event
    else {
        panic!("foreign MovePlayer was not routed to the actor stream");
    };

    assert!((remote.position[1].unwrap() - (SPAWN_FEET_Y + PLAYER_NETWORK_OFFSET)).abs() < 1e-5);
    assert_eq!(remote.position_origin, ActorPositionOrigin::NetworkOffset);
}

#[test]
fn server_authoritative_correction_bypasses_foreign_player_runtime_filter() {
    let mut sequencer = NetworkSequencer::new(7, 0, 42);
    let correction = WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
        position: [27.5, 111.0, 91.5],
        delta: [0.0; 3],
        pitch: -15.0,
        yaw: 90.0,
        subject: protocol::MovementCorrectionSubject::Player,
        on_ground: true,
        tick: 55,
    });

    assert!(matches!(
        sequencer.wrap(correction).event,
        WorldEvent::PlayerMovementCorrection(_)
    ));
}
