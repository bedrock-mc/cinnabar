use super::*;

fn skin(value: u8) -> protocol::PlayerSkin {
    protocol::PlayerSkin::Standard(protocol::StandardSkin {
        width: protocol::CLASSIC_SKIN_SIDE as u32,
        height: protocol::CLASSIC_SKIN_SIDE as u32,
        rgba8: vec![value; protocol::CLASSIC_SKIN_SIDE * protocol::CLASSIC_SKIN_SIDE * 4].into(),
        cape: None,
        geometry: None,
    })
}

#[test]
fn local_skin_selection_updates_authoritative_roster_without_replacing_identity() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 7,
            local_player_runtime_id: 41,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    let uuid = [9; 16];
    authority
        .apply_ordered_event(
            WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Add {
                        uuid,
                        unique_id: 7,
                        username: "signed-in".into(),
                        verified: true,
                        skin: skin(1),
                    }]),
                },
            )),
            Some(1),
        )
        .unwrap();
    let feed = LocalPlayerFeed {
        uuid: [5; 16],
        username: "local".into(),
        skin: skin(2),
        prefer_client_skin: false,
        position: [0.0; 3],
        velocity: [0.0; 3],
        on_ground: true,
        flying: false,
        gliding: false,
        fall_fly_ticks: 0,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: crate::ACTOR_SWING_TICKS,
        java_swing_ticks: crate::ACTOR_SWING_TICKS,
        teleported: false,
        first_person: false,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    };
    authority.sync_local_player_pose(&feed);
    assert!(authority.update_local_player_skin(skin(3)));
    let profile = authority.actors.player_profile(41).unwrap();
    assert_eq!(profile.skin, skin(3));
    assert!(
        matches!(&authority.actor(41).unwrap().kind, protocol::ActorKind::Player { uuid: actor_uuid, .. } if *actor_uuid == uuid)
    );
    assert_eq!(&*profile.username, "signed-in");
    assert!(profile.verified);
    authority.sync_local_player_pose(&feed);
    assert_eq!(authority.actors.player_profile(41).unwrap().skin, skin(3));
}
