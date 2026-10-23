//! The steered boat's prediction lands before this frame's picks are prepared.

use std::sync::Arc;

use chunk_pipeline::WorldStream;
use client_world::{LocalItemUse, LocalPlayerFeed, LocalVehicleFeed, VehicleRevision};
use protocol::{
    ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorSpawnEvent, PlayerSkin,
    PlayerSkinUnavailable, WorldBootstrap, WorldEvent,
};

use super::{ActorFrameState, advance_actor_motion};

const BOAT: u64 = 50;

fn rider() -> LocalPlayerFeed {
    LocalPlayerFeed {
        held_variants: Default::default(),
        game_mode: None,
        prefer_client_skin: false,
        uuid: [1; 16],
        username: "test".into(),
        skin: PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions),
        position: [10.0, 64.0, 10.0],
        velocity: [0.0; 3],
        on_ground: true,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        equipment_count: 0,
        held_network_ids: [0; 2],
        main_hand_metadata: 0,
        main_hand_slot: 0,
        main_hand_stack_id: None,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        flying: false,
        gliding: false,
        spinning: false,
        fall_fly_ticks: 0,
        teleported: false,
        first_person: true,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: LocalItemUse::Unpredicted,
    }
}

/// A stream whose local player steers a boat at x = 10.
fn steering() -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [10.0, 64.0, 10.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream.sync_local_player_pose(&rider());
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: BOAT as i64,
                runtime_id: BOAT,
                kind: ActorKind::Entity {
                    identifier: "minecraft:boat".into(),
                },
                position: [10.0, 64.0, 10.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::ActorLink(ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: BOAT as i64,
                rider_unique_id: 1,
                link_type: ActorLinkType::Rider,
                immediate: false,
                rider_initiated: false,
            }),
        )
        .unwrap();
    stream
}

#[test]
fn picks_read_the_steered_boat_this_frame_predicts() {
    let mut stream = steering();
    let feet = stream.authority().actor(BOAT).unwrap().position;
    let moved = [feet[0] + 2.0, feet[1], feet[2]];
    let mut state = ActorFrameState::default();
    advance_actor_motion(
        &mut state,
        Some(&mut stream),
        std::time::Duration::from_millis(60),
        Some(LocalVehicleFeed {
            runtime_id: BOAT,
            revision: VehicleRevision::default(),
            position: moved,
            yaw: 0.0,
            velocity: [0.0; 3],
            render_position: moved,
            render_yaw: 0.0,
        }),
    );
    let authority = stream.authority();
    let (min, max) = authority
        .pick_hit_boxes(authority.actor(BOAT).unwrap())
        .next()
        .unwrap();
    assert!(((min[0] + max[0]) / 2.0 - moved[0]).abs() < 1.0e-3);
}
