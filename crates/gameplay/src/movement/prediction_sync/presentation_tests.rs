use super::{
    LocalPhysicsController, send_movement_prediction_sync,
    tests::{LocalActor, due_physics},
};
use client_world::{LocalItemUse, LocalPlayerFeed, WorldAuthority};
use protocol::{ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue};
use protocol_ids::{ActorDataKey, ActorFlag};
use std::sync::Arc;

/// Creates a local presentation feed with no accepted gameplay use or locomotion.
fn feed() -> LocalPlayerFeed {
    LocalPlayerFeed {
        game_mode: None,
        uuid: [0; 16],
        username: "local".into(),
        skin: protocol::PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        prefer_client_skin: false,
        position: [0.0; 3],
        velocity: [0.0; 3],
        on_ground: true,
        flying: false,
        gliding: false,
        spinning: false,
        fall_fly_ticks: 0,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        equipment_count: 0,
        held_network_ids: [0; 2],
        main_hand_metadata: 0,
        held_variants: Default::default(),
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        teleported: false,
        first_person: false,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: LocalItemUse::Unpredicted,
    }
}

/// Emits one due packet from the actor snapshot gameplay borrows from authority.
fn sync(
    authority: &WorldAuthority,
    physics: &mut LocalPhysicsController,
    accepted: bool,
) -> protocol::Packet {
    let actor = LocalActor(authority.actor(1).unwrap().clone());
    let mut sent = None;
    send_movement_prediction_sync(physics, &actor, None, None, false, |packet| {
        sent = Some(packet);
        accepted
    });
    sent.expect("due correction sync")
}

/// Creates the stream authority queried by the local prediction sender.
fn authority() -> WorldAuthority {
    WorldAuthority::new(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::air_network_id(false),
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    )
}

/// Receives all flag words without passing through the local animation feed.
fn receive_flags(authority: &mut WorldAuthority, sequence: u64, words: [u64; 3]) {
    let metadata = [
        ActorDataKey::Flags,
        ActorDataKey::FlagsTwo,
        ActorDataKey::FlagsThird,
    ]
    .into_iter()
    .zip(words)
    .map(|(key, value)| ActorMetadata {
        key: key.wire_value(),
        value: if key == ActorDataKey::Flags {
            ActorMetadataValue::Flags(value)
        } else {
            ActorMetadataValue::FlagsExtended(value)
        },
    })
    .collect::<Vec<_>>();
    authority
        .apply_ordered_event(
            protocol::WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: metadata.into(),
                properties: [].into(),
                tick: sequence,
            })),
            Some(sequence),
        )
        .unwrap();
}

#[test]
fn presentation_only_use_cannot_change_emitted_prediction_sync() {
    let mut authority = authority();
    let mut feed = feed();
    authority.sync_local_player_pose(&feed);
    receive_flags(&mut authority, 1, [0; 3]);
    let mut physics = due_physics();
    let before = sync(&authority, &mut physics, false);
    // The Java sword pose requests visual use without admitting an item use.
    feed.item_use = LocalItemUse::Using;
    authority.sync_local_player_pose(&feed);
    assert!(
        authority.actor(1).unwrap().is_using_item(),
        "retain the rendered pose"
    );
    assert_eq!(
        sync(&authority, &mut physics, true),
        before,
        "presentation cannot author a correction packet"
    );
}

#[test]
fn presentation_changes_preserve_new_server_flags_on_a_sync_retry() {
    let mut authority = authority();
    let mut feed = feed();
    authority.sync_local_player_pose(&feed);
    receive_flags(&mut authority, 1, [0; 3]);
    let mut physics = due_physics();
    let before = sync(&authority, &mut physics, false);
    assert!(physics.prediction_sync.due());
    let words = [
        (1 << ActorFlag::Eating.wire_value()) | (1 << ActorFlag::UsingItem.wire_value()),
        (1 << (ActorFlag::Blocking.wire_value() - u64::BITS))
            | (1 << (ActorFlag::InUI.wire_value() - u64::BITS)),
        1 << (u64::BITS - 1),
    ];
    receive_flags(&mut authority, 2, words);
    let expected = sync(&authority, &mut due_physics(), true);
    assert_ne!(
        expected, before,
        "the retry must read newly accepted metadata"
    );
    feed.item_use = LocalItemUse::Idle;
    feed.gliding = true;
    feed.spinning = true;
    authority.sync_local_player_pose(&feed);
    assert!(!authority.actor(1).unwrap().is_using_item());
    assert_eq!(sync(&authority, &mut physics, true), expected);
    assert!(!physics.prediction_sync.due());
}
