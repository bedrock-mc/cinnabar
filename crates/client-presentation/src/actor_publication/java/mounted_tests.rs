use super::*;
use bevy::math::Quat;
use protocol::{ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorSpawnEvent, WorldEvent};

const MOUNT: u64 = 40;

#[test]
fn mounted_remote_frame_matches_fixed_native_java_states() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/mounted.json")).unwrap();
    let mut stream = super::tests::head_stream();
    stream.sync_local_player_pose(&feed(0.0));
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let source_rig = stream.authority().actor_rig(1).unwrap();
    for state in fixture["states"].as_array().unwrap() {
        let input: [f32; 9] =
            std::array::from_fn(|index| state["input"][index].as_f64().unwrap() as f32);
        let expected: [f32; 4] = std::array::from_fn(|index| {
            state["body_head_pitch_relative"][index].as_f64().unwrap() as f32
        });
        let mut actor = stream.authority().actor(1).unwrap().clone();
        actor.previous_pose.head_yaw = input[2];
        actor.head_yaw = input[3];
        actor.previous_pose.pitch = input[6];
        actor.pitch = input[7];
        actor.received_pose.head_yaw = 25.0;
        actor.received_pose.pitch = -40.0;
        let alpha = input[8];
        let (head, _) = head_look(&actor, alpha, false);
        let body = mounted::frame_body_yaw([input[4], input[5]], head, alpha);
        let mut rig = ActorRigSnapshot {
            previous_body_yaw: body,
            body_yaw: body,
            ..source_rig
        };
        rig.java.body_yaw = [input[0], input[1]];
        let pose = third_person_input(&rig, &actor, None, alpha, false);
        degrees_near(body, expected[0]);
        degrees_near(head, expected[1]);
        assert!((pose.head_pitch - expected[2]).abs() < 1e-5);
        degrees_near(pose.head_yaw, expected[3]);
    }
}

fn mounted_stream(identifier: &str, mount_yaw: f32) -> WorldStream {
    let entity = |identifier: &str| {
        format!(r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"{identifier}","materials":{{"default":"entity"}},"textures":{{"default":"textures/entity/test"}},"geometry":{{"default":"geometry.mounted"}},"render_controllers":["controller.render.mounted"]}}}}}}"#).into_bytes()
    };
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.mounted","texture_width":64,"texture_height":64},"bones":[{"name":"head","pivot":[0,24,0]},{"name":"body","pivot":[0,24,0]},{"name":"rightarm","pivot":[5,22,0]},{"name":"leftarm","pivot":[-5,22,0]},{"name":"rightleg","pivot":[1.9,12,0]},{"name":"leftleg","pivot":[-1.9,12,0]}]}]}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.mounted":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let compiled = pack_compiler::compile_entity_pack(vec![
        ("entity/player.json".into(), entity("minecraft:player")),
        ("entity/mount.json".into(), entity(identifier)),
        ("models/entity/mounted.geo.json".into(), geometry.to_vec()),
        (
            "render_controllers/mounted.json".into(),
            controller.to_vec(),
        ),
    ])
    .unwrap()
    .unwrap();
    let mut stream = WorldStream::new_with_asset_sets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap()),
        [0.0, 64.0, 0.0],
        None,
    );
    stream.sync_local_player_pose(&feed(0.0));
    stream.prepare_actor_appearance_fixture();
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: MOUNT as i64,
                runtime_id: MOUNT,
                kind: ActorKind::Entity {
                    identifier: Arc::from(identifier),
                },
                position: [0.0, 64.0, 0.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: mount_yaw,
                head_yaw: mount_yaw,
                body_yaw: mount_yaw,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    link(&mut stream, 2, ActorLinkType::Rider);
    stream.advance_actor_interpolation_frame(1);
    stream
}

fn feed(head_yaw: f32) -> client_world::LocalPlayerFeed {
    let mut feed = super::tests::head_feed();
    feed.yaw = head_yaw;
    feed.head_yaw = head_yaw;
    feed.pitch = 0.0;
    feed
}

fn link(stream: &mut WorldStream, sequence: u64, link_type: ActorLinkType) {
    stream
        .submit(
            sequence,
            WorldEvent::ActorLink(ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: MOUNT as i64,
                rider_unique_id: 1,
                link_type,
                immediate: false,
                rider_initiated: false,
            }),
        )
        .unwrap();
}

fn frame(stream: &WorldStream, local: bool, alpha: f32) -> ThirdPerson<'_> {
    let rig = stream.authority().actor_rig(1).unwrap();
    let actor = stream.authority().actor(1).unwrap();
    let equipment = ActorEquipmentInput::default();
    third_person(
        stream,
        &rig,
        actor,
        local.then_some(&equipment),
        alpha,
        &mut PoseScratch::default(),
    )
    .unwrap()
}

fn degrees_near(actual: f32, expected: f32) {
    assert!(
        wrap_degrees(actual - expected).abs() < 1e-4,
        "{actual} != {expected}"
    );
}

fn head_near(posed: &ThirdPerson<'_>, expected: f32) {
    head_pose_near(posed, expected, 0.0);
}

fn head_pose_near(posed: &ThirdPerson<'_>, yaw: f32, pitch: f32) {
    let index = posed
        .rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "head")
        .unwrap();
    let actual = Quat::from_array(posed.bones[index].rotation);
    let expected =
        Quat::from_rotation_y(-yaw.to_radians()) * Quat::from_rotation_x(-pitch.to_radians());
    assert!(
        actual.dot(expected).abs() > 1.0 - 1e-6,
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn remote_mounted_head_interpolates_tick_angles_and_ignores_the_pending_target() {
    let mut stream = mounted_stream("minecraft:horse", 10.0);
    stream
        .submit(
            3,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Player {
                    uuid: [2; 16],
                    username: Arc::from("Remote rider"),
                },
                position: [0.0, 64.0, 0.0],
                velocity: [0.0; 3],
                pitch: 25.0,
                yaw: 170.0,
                head_yaw: 170.0,
                body_yaw: 170.0,
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
            4,
            WorldEvent::ActorLink(ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: MOUNT as i64,
                rider_unique_id: 2,
                link_type: ActorLinkType::Rider,
                immediate: false,
                rider_initiated: false,
            }),
        )
        .unwrap();
    stream.advance_actor_interpolation_frame(1);
    for (sequence, runtime_id, pitch, yaw) in [(5, MOUNT, None, 70.0), (6, 2, Some(55.0), -130.0)] {
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
                    dimension: 0,
                    runtime_id,
                    position: [Some(0.2), None, None],
                    position_origin: protocol::ActorPositionOrigin::Feet,
                    pitch,
                    yaw: Some(yaw),
                    head_yaw: Some(yaw),
                    on_ground: Some(true),
                    teleported: false,
                    player_mode: None,
                    source_tick: None,
                    interpolation: protocol::ActorInterpolation {
                        ticks: 3,
                        force_completion: false,
                    },
                })),
            )
            .unwrap();
    }
    stream.advance_actor_interpolation_frame(1);
    let mount = stream.authority().actor_rig(MOUNT).unwrap();
    degrees_near(mount.previous_body_yaw, 10.0);
    degrees_near(mount.body_yaw, 30.0);
    let actor = stream.authority().actor(2).unwrap();
    degrees_near(actor.previous_pose.head_yaw, 170.0);
    degrees_near(actor.head_yaw, -170.0);
    assert_eq!(actor.previous_pose.pitch, 25.0);
    assert_eq!(actor.pitch, 35.0);
    assert_ne!(actor.head_yaw, actor.received_pose.head_yaw);
    let rig = stream.authority().actor_rig(2).unwrap();
    let posed = third_person(&stream, &rig, actor, None, 0.5, &mut PoseScratch::default()).unwrap();
    degrees_near(posed.rig.body_yaw, 112.0);
    head_pose_near(&posed, 68.0, 30.0);
    degrees_near(
        posed.posed.cape.body_yaw,
        lerp_degrees(rig.java.body_yaw[0], rig.java.body_yaw[1], 0.5),
    );
}

#[test]
fn living_mount_uses_its_render_yaw_and_soft_head_limit_with_live_local_look() {
    for identifier in [
        "minecraft:horse",
        "minecraft:pig",
        "minecraft:camel",
        "minecraft:llama",
        "minecraft:strider",
    ] {
        let mut stream = mounted_stream(identifier, 0.0);
        let original = stream.authority().actor_rig(1).unwrap().java;
        for (look, body, relative) in [
            (120.0, 52.0, 68.0),
            (-120.0, -52.0, -68.0),
            (90.0, 22.0, 68.0),
            (-90.0, -22.0, -68.0),
            (50.0, 0.0, 50.0),
            (51.0, 10.2, 40.8),
            (40.0, 0.0, 40.0),
        ] {
            stream.sync_local_player_pose(&feed(look));
            stream.prepare_actor_appearance_fixture();
            stream.advance_actor_interpolation_frame(0);
            let actor_before = stream.authority().actor(1).unwrap().clone();
            let posed = frame(&stream, true, 0.5);
            degrees_near(
                lerp_degrees(posed.rig.previous_body_yaw, posed.rig.body_yaw, 0.5),
                body,
            );
            head_near(&posed, relative);
            assert_eq!(stream.authority().actor(1).unwrap(), &actor_before);
            assert_eq!(stream.authority().actor_rig(1).unwrap().java, original);
            degrees_near(posed.posed.cape.body_yaw, 0.0);
        }
    }
}

#[test]
fn mount_yaw_interpolates_across_the_wrap_without_changing_cape_yaw() {
    let mut stream = mounted_stream("minecraft:horse", 179.0);
    stream
        .submit(
            3,
            WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
                dimension: 0,
                runtime_id: MOUNT,
                position: [Some(0.2), None, None],
                position_origin: protocol::ActorPositionOrigin::Feet,
                pitch: None,
                yaw: Some(-179.0),
                head_yaw: Some(-179.0),
                on_ground: Some(true),
                teleported: false,
                player_mode: None,
                source_tick: None,
                interpolation: protocol::ActorInterpolation {
                    ticks: 3,
                    force_completion: false,
                },
            })),
        )
        .unwrap();
    stream.sync_local_player_pose(&feed(175.0));
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let mount = stream.authority().actor_rig(MOUNT).unwrap();
    assert!(wrap_degrees(mount.body_yaw - mount.previous_body_yaw) > 0.0);
    let motion = stream.authority().actor_rig(1).unwrap().java;
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let expected = lerp_degrees(mount.previous_body_yaw, mount.body_yaw, alpha);
        let posed = frame(&stream, true, alpha);
        degrees_near(
            lerp_degrees(posed.rig.previous_body_yaw, posed.rig.body_yaw, alpha),
            expected,
        );
        head_near(&posed, wrap_degrees(175.0 - expected));
        degrees_near(
            posed.posed.cape.body_yaw,
            lerp_degrees(motion.body_yaw[0], motion.body_yaw[1], alpha),
        );
    }
}

#[test]
fn nonliving_unknown_and_removed_mounts_keep_the_ordinary_player_body_basis() {
    for identifier in [
        "minecraft:boat",
        "minecraft:chest_boat",
        "minecraft:minecart",
        "fixture:unknown_mount",
    ] {
        let mut stream = mounted_stream(identifier, 0.0);
        stream.sync_local_player_pose(&feed(120.0));
        stream.prepare_actor_appearance_fixture();
        stream.advance_actor_interpolation_frame(1);
        let rig = stream.authority().actor_rig(1).unwrap();
        let expected = lerp_degrees(rig.java.body_yaw[0], rig.java.body_yaw[1], 0.5);
        let posed = frame(&stream, true, 0.5);
        degrees_near(
            lerp_degrees(posed.rig.previous_body_yaw, posed.rig.body_yaw, 0.5),
            expected,
        );
        head_near(&posed, wrap_degrees(120.0 - expected));
    }
    let mut stream = mounted_stream("minecraft:horse", 0.0);
    stream.sync_local_player_pose(&feed(120.0));
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    link(&mut stream, 3, ActorLinkType::Remove);
    stream.advance_actor_interpolation_frame(1);
    let rig = stream.authority().actor_rig(1).unwrap();
    assert!(!rig.java.riding);
    let expected = lerp_degrees(rig.java.body_yaw[0], rig.java.body_yaw[1], 0.5);
    let posed = frame(&stream, true, 0.5);
    degrees_near(
        lerp_degrees(posed.rig.previous_body_yaw, posed.rig.body_yaw, 0.5),
        expected,
    );
}

/// Local torso headings use physical interpolation while cape position keeps actor interpolation.
#[test]
fn local_java_torso_frame_uses_physics_alpha_for_heading_and_cape_rotation() {
    let mut stream = super::tests::head_stream();
    let mut feed = super::tests::uploaded_skin_feed(false);
    feed.yaw = 30.0;
    feed.head_yaw = 30.0;
    feed.pitch = 0.0;
    stream.sync_local_player_pose(&feed);
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let mut rig = stream.authority().actor_rig(1).unwrap();
    rig.java.body_yaw = [10.0, 30.0];
    rig.java.body_frame_alpha = Some(0.75);
    rig.java.cape = [[0.0; 3], [10.0, 20.0, 30.0]];
    let actor = stream.authority().actor(1).unwrap();
    let equipment = ActorEquipmentInput::default();
    let posed = third_person(
        &stream,
        &rig,
        actor,
        Some(&equipment),
        0.1,
        &mut PoseScratch::default(),
    )
    .unwrap();
    degrees_near(
        lerp_degrees(posed.rig.previous_body_yaw, posed.rig.body_yaw, 0.1),
        25.0,
    );
    degrees_near(posed.posed.cape.body_yaw, 25.0);
    assert!(
        posed
            .posed
            .cape
            .chase
            .abs_diff_eq(Vec3::new(1.0, 2.0, 3.0), 1e-5)
    );
    head_pose_near(&posed, 5.0, 0.0);
}

/// A changing physical fraction cannot invalidate a hand source when the torso is still.
#[test]
fn local_java_torso_phase_without_motion_retains_the_same_hand_source() {
    let mut stream = super::tests::head_stream();
    let mut feed = super::tests::uploaded_skin_feed(false);
    feed.yaw = 0.0;
    feed.head_yaw = 0.0;
    stream.sync_local_player_pose(&feed);
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    stream.set_local_motion_authority(Some((1, 1)));
    let sample = |alpha| client_world::LocalSwingMotionSample {
        tick: 1,
        delta: [0.0; 3],
        yaw: 0.0,
        progress: client_world::LocalSwingProgress {
            frame_alpha: Some(alpha),
            ..Default::default()
        },
    };
    stream.sync_local_swing_motion((1, 1), [sample(0.25)]);
    let first = super::super::hand::source_key(&stream, None, None, 0.1);
    assert!(first.is_some());
    stream.sync_local_swing_motion((1, 1), [sample(0.75)]);
    let second = super::super::hand::source_key(&stream, None, None, 0.1);
    assert!(
        first == second,
        "unchanged sampled torso keeps the hand source"
    );
}
