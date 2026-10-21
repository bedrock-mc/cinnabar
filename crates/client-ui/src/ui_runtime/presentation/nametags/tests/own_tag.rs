use super::*;
use protocol_ids::ActorFlag;

/// Supplies a client-owned player without any server-authored actor metadata.
fn local_feed() -> client_world::LocalPlayerFeed {
    client_world::LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        uuid: [0; 16],
        username: "Player".into(),
        skin: protocol::PlayerSkin::Standard(render_api::StandardSkin {
            geometry: None,
            cape: None,
            width: 64,
            height: 64,
            rgba8: vec![0; 64 * 64 * 4].into(),
        }),
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
        held_network_ids: [0; 2],
        held_variants: Default::default(),
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        teleported: false,
        first_person: false,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    }
}

#[test]
fn own_tag_eligibility_uses_server_metadata_and_local_pose() {
    let mut stream = actor_stream(0, None, 1);
    let mut feed = local_feed();
    let boards = ui::ScoreboardStore::default();
    stream.sync_local_player_pose(&feed);
    let own_tag = |stream: &chunk_pipeline::WorldStream| {
        let authority = stream.authority();
        let actor = authority
            .actor(authority.local_player_runtime_id())
            .unwrap();
        extract_nametag(
            actor,
            view(Vec3::ZERO),
            None,
            authority.actor_name_tag(actor.unique_id).unwrap(),
            &boards,
            1.0,
        )
    };
    assert!(own_tag(&stream).is_none());
    stream
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    metadata: Arc::from([protocol::ActorMetadata {
                        key: METADATA_ALWAYS_SHOW_NAMETAG,
                        value: ActorMetadataValue::Byte(1),
                    }]),
                    properties: Arc::from([]),
                    tick: 0,
                },
            )),
        )
        .unwrap();
    stream.poll([0.0; 3], 0);
    assert!(own_tag(&stream).is_some());
    assert!(
        project_nametags(
            &mut NametagTextCache::default(),
            &boards,
            &stream,
            view(Vec3::ZERO),
            None,
            1.0,
            true,
            |_| true
        )
        .is_empty()
    );
    feed.sneaking = true;
    stream.sync_local_player_pose(&feed);
    assert!(own_tag(&stream).unwrap().depth_tested);
    assert!(
        crate::ui_runtime::presentation::player_preview::PlayerPreviewPose::of_local_player(Some(
            &stream
        ))
        .sneaking
    );
    for (index, (always, flags, expected)) in [
        (0, 1 << ActorFlag::ShowName.wire_value(), false),
        (-1, 1 << ActorFlag::ShowName.wire_value(), false),
        (1, 0, true),
        (
            1,
            (1 << ActorFlag::ShowName.wire_value()) | (1 << ActorFlag::Invisible.wire_value()),
            false,
        ),
        (1, 1 << ActorFlag::ShowName.wire_value(), true),
    ]
    .into_iter()
    .enumerate()
    {
        stream
            .submit(
                index as u64 + 2,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                    protocol::ActorMetadataUpdateEvent {
                        dimension: 0,
                        runtime_id: 1,
                        metadata: Arc::from([
                            protocol::ActorMetadata {
                                key: METADATA_ALWAYS_SHOW_NAMETAG,
                                value: ActorMetadataValue::Byte(always),
                            },
                            protocol::ActorMetadata {
                                key: ActorDataKey::Flags.wire_value(),
                                value: ActorMetadataValue::Flags(flags),
                            },
                        ]),
                        properties: Arc::from([]),
                        tick: 0,
                    },
                )),
            )
            .unwrap();
        stream.poll([0.0; 3], 0);
        stream.sync_local_player_pose(&feed);
        assert_eq!(own_tag(&stream).is_some(), expected);
    }
}

#[test]
fn render_feet_replace_stale_actor_interpolation_for_geometry_and_score_range() {
    let mut actor = actor();
    actor.previous_pose.position = [0.0, 62.0, 0.0];
    actor.received_pose.position = [2.0, 64.0, 0.0];
    actor.position = actor.received_pose.position;
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("7 wins".into()));
    let boards = ui::ScoreboardStore::default();
    for feet in [
        Vec3::new(30.0, 65.0, 10.0),
        Vec3::new(29.0, 66.0, 8.0),
        Vec3::new(-24.0, 62.0, -10.0),
    ] {
        let mut camera = view(feet);
        camera.camera_position = feet + Vec3::new(0.0, 2.0, 4.0);
        let tag =
            extract_nametag_at_feet(&actor, camera, None, "Player".into(), Some(&boards), feet)
                .unwrap();
        assert_eq!(tag.position, feet + Vec3::Y * tag_height(&actor));
        assert_eq!(tag.lines[1].as_ref(), "7 wins");
        let old_tag = extract_nametag(&actor, camera, None, "Player".into(), &boards, 0.5).unwrap();
        assert_ne!(tag.position, old_tag.position);
        assert_eq!(
            old_tag.lines.len(),
            1,
            "remote score range uses interpolated actor feet"
        );
        assert_eq!(
            tag.view_depth,
            camera
                .camera_forward
                .dot(tag.position - camera.camera_position)
        );
    }
}

#[test]
fn render_feet_preserve_actor_visibility_height_and_reject_nonfinite_positions() {
    let mut actor = actor();
    let feet = Vec3::new(8.0, 70.0, -3.0);
    actor
        .metadata
        .insert(METADATA_HEIGHT, ActorMetadataValue::Float(3.0));
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(
            (1 << ActorFlag::ShowName.wire_value()) | (1 << ActorFlag::Sneaking.wire_value()),
        ),
    );
    let boards = ui::ScoreboardStore::default();
    let extract = |actor: &ActorSnapshot, position| {
        extract_nametag_at_feet(
            actor,
            view(feet),
            None,
            "Player".into(),
            Some(&boards),
            position,
        )
    };
    let tag = extract(&actor, feet).unwrap();
    assert_eq!(tag.position, feet + Vec3::Y * tag_height(&actor));
    assert!(tag.depth_tested);
    for invalid in [
        Vec3::new(f32::NAN, 70.0, -3.0),
        Vec3::new(8.0, f32::INFINITY, -3.0),
        Vec3::new(8.0, 70.0, f32::NEG_INFINITY),
    ] {
        assert!(extract(&actor, invalid).is_none());
    }
    actor.metadata.insert(0, ActorMetadataValue::Flags(0));
    assert!(extract(&actor, feet).is_some());
    actor
        .metadata
        .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(0));
    assert!(extract(&actor, feet).is_none());
    actor
        .metadata
        .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(1));
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::Invisible.wire_value()),
    );
    assert!(extract(&actor, feet).is_none());
}
