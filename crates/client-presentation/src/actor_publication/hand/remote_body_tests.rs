//! Remote native body and persona bones reach the submitted draw without replacing Java poses.
use super::*;
use bevy::math::Quat;

/// Adds two remote players sharing skin data but using opposite head rotations, standing in a
/// resident column so their moves interpolate instead of snapping as moves into unloaded terrain.
fn spawn_remotes(stream: &mut WorldStream) {
    let skin = stream
        .authority()
        .actor_player_profile(1)
        .unwrap()
        .skin
        .clone();
    let mut payload = vec![1, 2];
    payload.extend(std::iter::repeat_n(
        0xff,
        protocol::vanilla_dimension_range(0)
            .unwrap()
            .sub_chunk_count
            - 1,
    ));
    payload.push(0);
    stream
        .submit(
            1,
            protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                payload,
            }),
        )
        .unwrap();
    test_time::eventually("resident remote column", || {
        stream.poll([2.5, 64.0, 0.5], 0);
        stream.committed_sequence() >= 1
    });
    let mut sequence = 2;
    for id in [2u64, 3] {
        stream
            .submit(
                sequence,
                protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                    protocol::PlayerListUpdateEvent {
                        entries: Arc::from([protocol::PlayerListEntry::Add {
                            uuid: [id as u8; 16],
                            unique_id: id as i64,
                            username: "Remote".into(),
                            verified: true,
                            skin: skin.clone(),
                        }]),
                    },
                )),
            )
            .unwrap();
        sequence += 1;
        stream
            .submit(
                sequence,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(
                    protocol::ActorSpawnEvent {
                        dimension: 0,
                        unique_id: id as i64,
                        runtime_id: id,
                        kind: protocol::ActorKind::Player {
                            uuid: [id as u8; 16],
                            username: "Remote".into(),
                        },
                        position: [id as f32, 64.0, 0.0],
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
                    },
                )),
            )
            .unwrap();
        sequence += 1;
        assert!(
            stream.authority().actor(id).is_some(),
            "remote spawn must commit"
        );
    }
    stream.prepare_actor_appearance_fixture();
    for id in [2u64, 3] {
        // Profiles publish only once their prepared appearance is ready.
        assert!(
            stream.authority().actor_player_profile(id).is_some(),
            "remote profile must commit"
        );
    }
    stream.advance_actor_interpolation_frame(1);
    for id in [2u64, 3] {
        assert!(
            stream
                .authority()
                .actor_rig(id)
                .expect("committed remote rig")
                .pose_evaluated
        );
        stream
            .submit(
                sequence,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Move(protocol::ActorMoveEvent {
                    dimension: 0,
                    runtime_id: id,
                    position: [None; 3],
                    position_origin: protocol::ActorPositionOrigin::NetworkOffset,
                    pitch: None,
                    yaw: None,
                    head_yaw: Some(if id == 2 { 90.0 } else { -90.0 }),
                    on_ground: Some(true),
                    teleported: false,
                    player_mode: None,
                    source_tick: None,
                    interpolation: protocol::ActorInterpolation {
                        ticks: 1,
                        force_completion: false,
                    },
                })),
            )
            .unwrap();
        sequence += 1;
    }
    // Retain two evaluated samples after the appearance warmup.
    stream.advance_actor_interpolation_ticks(2);
}

#[test]
fn remote_body_and_persona_publish_sampled_clamps_and_preserve_java_overrides() {
    for animated in [false, true] {
        let ((mut stream, mut equipment, artwork, local_equipment), entities) =
            super::native_tests::fixture_with_rotation(animated);
        spawn_remotes(&mut stream);
        for java_mode in [false, true] {
            for alpha in [0.05, 0.75] {
                let input = super::super::ActorFrameInput {
                    local_feed: None,
                    predicted_eye: None,
                    predicted_feet: None,
                    local_equipment: local_equipment.clone(),
                    swing_progress: None,
                    renders_game: true,
                    hide_hand: false,
                    custom_emote: None,
                };
                let mut poses = crate::presentation::actors::PoseConversions::default();
                let mut java_poses = java::PoseScratch::default();
                let mut native_pose = NativePoseCache::default();
                let mut skin_rigs = crate::presentation::skin_rig::SkinRigCache::default();
                let mut geometries = Vec::new();
                let mut bodies = super::super::bodies::sample(
                    Some(&stream),
                    &input,
                    (
                        super::super::bodies::PoseView {
                            partial_tick: alpha,
                            first_person: true,
                            java_mode,
                            combat_visuals: Default::default(),
                            sampling_camera: None,
                            aspect: 1.0,
                        },
                        None,
                    ),
                    &artwork,
                    super::super::bodies::BodyCaches {
                        poses: &mut poses,
                        java_poses: &mut java_poses,
                        native_pose: &mut native_pose,
                        skin_rigs: &mut skin_rigs,
                        equipment: Some(&mut equipment),
                    },
                    |_, _, _, _| false,
                    &mut geometries,
                );
                assert_eq!(
                    bodies.remotes.len(),
                    2,
                    "both remote bodies must be drawable"
                );
                let mut batch = crate::presentation::actors::select_actor_presentations(
                    1,
                    false,
                    None,
                    std::mem::take(&mut bodies.remotes),
                );
                let mut layer_poses = crate::presentation::entity_layers::LayerPoseCache::default();
                let mut cape = crate::presentation::cape::CapeState::default();
                let mut skin_layers = crate::presentation::skin_layers::SkinLayerCache::default();
                let mut scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
                super::super::layers::attach(
                    &mut batch,
                    Some(&stream),
                    bodies.render_frame.as_mut(),
                    super::super::layers::LayerPoses {
                        local_runtime_id: 1,
                        local_equipment: &local_equipment,
                        java_posed: &bodies.java_posed,
                        combat_visuals: Default::default(),
                        local_emote_pose: None,
                        native_body_sampled: false,
                        native_pose: &native_pose,
                        remote_skin_layers: &bodies.remote_skin_layers,
                    },
                    super::super::layers::LayerCaches {
                        layer_poses: &mut layer_poses,
                        equipment: None,
                        cape_state: &mut cape,
                        skin_layers: &mut skin_layers,
                        skin_rigs: &mut skin_rigs,
                    },
                    &artwork,
                    Some(&entities),
                    &mut scene,
                    (alpha, 0.0, None),
                    &mut geometries,
                );
                for id in [2u64, 3] {
                    let rig = stream.authority().actor_rig(id).unwrap();
                    let actor = stream.authority().actor(id).unwrap();
                    let body = batch
                        .submissions
                        .iter()
                        .find(|draw| {
                            draw.input.identity.runtime_id == id
                                && draw.input.identity.layer == render_model::ACTOR_LAYER_BODY
                        })
                        .unwrap();
                    let right = rig
                        .bone_names
                        .iter()
                        .position(|name| name.as_ref() == "rightarm")
                        .unwrap();
                    if java_mode {
                        let expected = java::third_person(
                            &stream,
                            &rig,
                            actor,
                            None,
                            alpha,
                            &mut java::PoseScratch::default(),
                        )
                        .unwrap();
                        assert_eq!(body.input.previous_bones, expected.bones);
                        assert_eq!(body.input.current_bones, expected.bones);
                        if animated {
                            let layer = batch
                                .submissions
                                .iter()
                                .find(|draw| {
                                    draw.input.identity.runtime_id == id
                                        && crate::presentation::skin_layers::is_skin_layer(
                                            draw.input.identity.layer,
                                        )
                                })
                                .unwrap();
                            assert_eq!(
                                layer.input.current_bones,
                                crate::presentation::actors::convert_bones(
                                    &expected.posed.skin_layers[0].current
                                )
                                .unwrap()
                            );
                        }
                        continue;
                    }
                    let head = actor.previous_pose.head_yaw
                        + (actor.head_yaw - actor.previous_pose.head_yaw) * alpha;
                    let body_yaw =
                        rig.previous_body_yaw + (rig.body_yaw - rig.previous_body_yaw) * alpha;
                    let angle = (head - body_yaw).clamp(-10.0, 10.0);
                    let expected = Quat::from_rotation_y(-angle.to_radians());
                    for bones in [&body.input.previous_bones, &body.input.current_bones] {
                        assert!(
                            Quat::from_array(bones[right].rotation).abs_diff_eq(expected, 1e-5)
                        );
                    }
                    let left = rig
                        .bone_names
                        .iter()
                        .position(|name| name.as_ref() == "leftarm")
                        .unwrap();
                    let previous =
                        crate::presentation::actors::convert_bones(rig.previous).unwrap();
                    let current = crate::presentation::actors::convert_bones(rig.current).unwrap();
                    assert_ne!(previous[left], current[left]);
                    assert_eq!(body.input.previous_bones[left], previous[left]);
                    assert_eq!(body.input.current_bones[left], current[left]);
                    if animated {
                        let layer = batch
                            .submissions
                            .iter()
                            .find(|draw| {
                                draw.input.identity.runtime_id == id
                                    && crate::presentation::skin_layers::is_skin_layer(
                                        draw.input.identity.layer,
                                    )
                            })
                            .unwrap();
                        for bones in [&layer.input.previous_bones, &layer.input.current_bones] {
                            assert!(
                                Quat::from_array(bones[0].rotation).abs_diff_eq(expected, 1e-5)
                            );
                        }
                        assert_ne!(
                            layer.input.current_bones[0].translation_scale,
                            body.input.current_bones[right].translation_scale
                        );
                    }
                }
            }
        }
    }
}
