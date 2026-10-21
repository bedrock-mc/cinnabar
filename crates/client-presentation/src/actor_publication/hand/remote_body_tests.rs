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
                            xuid: "".into(),
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
                    cosmetics: Vec::new(),
                    local_feed: None,
                    predicted_eye: None,
                    predicted_feet: None,
                    local_equipment: local_equipment.clone(),
                    swing_progress: None,
                    renders_game: true,
                    hide_hand: false,
                    remote_emotes: Vec::new(),
                    remote_hats: Vec::new(),
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
                        scratch: &mut super::super::bodies::BodyScratch::default(),
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
                        remote_emote_poses: &bodies.remote_emote_poses,
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

#[test]
fn cloth_contacts_include_the_rendered_emote_spine_and_limb_segments() {
    let ((stream, _, _, _), _) = super::native_tests::fixture_with_rotation(true);
    let skin = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.cloth_emote"}}"#,
        r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{
            "identifier":"geometry.cloth_emote","texture_width":64,"texture_height":64},"bones":[
            {"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[0,0]}]},
            {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]},
            {"name":"leftarm","parent":"body","pivot":[5,22,0],"cubes":[{"origin":[4,12,-2],"size":[4,12,4],"uv":[0,0]}]},
            {"name":"rightarm","parent":"body","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[0,0]}]},
            {"name":"leftleg","pivot":[2,12,0],"cubes":[{"origin":[0,0,-2],"size":[4,12,4],"uv":[0,0]}]},
            {"name":"rightleg","pivot":[-2,12,0],"cubes":[{"origin":[-4,0,-2],"size":[4,12,4],"uv":[0,0]}]}]}]}"#,
    ).unwrap().unwrap();
    let skin = Arc::new(skin);
    let names: Vec<Box<str>> = skin.bones.iter().map(|bone| bone.name.clone()).collect();
    let rest: Vec<_> = skin
        .bones
        .iter()
        .map(|bone| {
            let pivot = bone.pivot.unwrap().map(|value| value.get() / 16.0);
            actor_animation::BoneTransform {
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation_scale: [pivot[0], pivot[1], pivot[2], 1.0],
                axis_scale: [1.0; 3],
            }
        })
        .collect();
    let original = actor_animation::ActorRigSnapshot {
        bone_names: &names,
        previous: &rest,
        current: &rest,
        rest: &rest,
        skin_geometry: Some(&skin),
        skin_mesh: None,
        skin_layers: &[],
        ..stream.authority().actor_rig(1).unwrap()
    };
    let pose = actor_animation::sample_custom_emote(
        &original,
        cosmetics::test_support::playback_emote(),
        0.25,
        0.3,
    )
    .unwrap();
    let poses = bevy::platform::collections::HashMap::from_iter([(1, pose)]);
    for local in [false, true] {
        let frame = super::super::hats::AttachmentFrame {
            local_emote: poses.get(&1),
            remote_emotes: Some(&poses),
            ..Default::default()
        };
        let rendered = frame.rig(original, local);
        let mesh = rendered.skin_mesh.unwrap();
        assert!(rendered.bone_names.len() > original.bone_names.len());
        let mut body = crate::presentation::actors::actor_rig_presentation(
            &rendered,
            stream.authority().actor(1).unwrap(),
            stream.authority().actor_player_profile(1),
            0.5,
        )
        .unwrap()
        .submission;
        body.input.rig = mesh.id;
        let contacts = super::super::cloth_contacts::collect(
            &body,
            rendered.bone_names,
            &[],
            &Default::default(),
            0.5,
            (std::slice::from_ref(mesh), None),
        );
        let expected = (0..rendered.bone_names.len())
            .filter(|&index| mesh.bone_bounds(index).is_some())
            .count();
        assert_eq!(
            contacts.len(),
            expected,
            "every rendered torso and limb segment must provide cloth contact"
        );
        assert!(contacts.len() > original.bone_names.len());
    }
}

#[test]
fn shared_emotes_override_only_the_matching_remote_lifetime_and_restore_native_or_java() {
    let ((mut stream, _, artwork, local_equipment), _) =
        super::native_tests::fixture_with_rotation(true);
    spawn_remotes(&mut stream);
    let rig = stream.authority().actor_rig(2).unwrap();
    let native = rig.current.to_vec();
    let revision = rig.actor.spawn_revision;
    let clip = cosmetics::test_support::playback_emote();
    for java_mode in [false, true] {
        let ordinary =
            sample_shared_bodies(&stream, &artwork, &local_equipment, java_mode, Vec::new());
        for phase in [0.0, 0.1, 0.3] {
            let expected = actor_animation::sample_custom_emote(&rig, clip, phase, phase).unwrap();
            let sampled = sample_shared_bodies(
                &stream,
                &artwork,
                &local_equipment,
                java_mode,
                vec![super::super::RemoteEmote {
                    runtime_id: 2,
                    spawn_revision: revision,
                    emote: clip,
                    elapsed: phase,
                }],
            );
            let find = |bodies: &super::super::bodies::SampledBodies<'_>, id| {
                bodies
                    .remotes
                    .iter()
                    .find(|body| body.submission.input.identity.runtime_id == id)
                    .unwrap()
                    .submission
                    .input
                    .current_bones
                    .clone()
            };
            assert_eq!(
                find(&sampled, 2),
                crate::presentation::actors::convert_bones(&expected.current).unwrap()
            );
            assert_eq!(
                find(&sampled, 3),
                find(&ordinary, 3),
                "other players keep their own animation"
            );
            assert_eq!(
                sampled
                    .canonical_local
                    .as_ref()
                    .unwrap()
                    .submission
                    .input
                    .current_bones,
                ordinary
                    .canonical_local
                    .as_ref()
                    .unwrap()
                    .submission
                    .input
                    .current_bones
            );
            let published = super::super::emote_geometry::skin_layer_snapshot(
                rig,
                sampled.remote_emote_poses.get(&2),
                None,
                None,
            );
            assert_eq!(
                published.skin_layers[0].current,
                expected.skin_layers[0].current
            );
            let stale = sample_shared_bodies(
                &stream,
                &artwork,
                &local_equipment,
                java_mode,
                vec![super::super::RemoteEmote {
                    runtime_id: 2,
                    spawn_revision: revision + 1,
                    emote: clip,
                    elapsed: phase,
                }],
            );
            assert_eq!(
                find(&stale, 2),
                find(&ordinary, 2),
                "a reused runtime ID cannot inherit playback"
            );
            assert!(stale.remote_emote_poses.is_empty());
        }
    }
    assert_eq!(stream.authority().actor_rig(2).unwrap().current, native);
}

/// Runs the production body sampler with an explicit leased remote playback set.
fn sample_shared_bodies<'a>(
    stream: &'a WorldStream,
    artwork: &render_model::ActorArtworkPages,
    local_equipment: &ActorEquipmentInput,
    java_mode: bool,
    remote_emotes: Vec<super::super::RemoteEmote>,
) -> super::super::bodies::SampledBodies<'a> {
    let input = super::super::ActorFrameInput {
        cosmetics: Vec::new(),
        local_feed: None,
        predicted_eye: None,
        predicted_feet: None,
        local_equipment: local_equipment.clone(),
        swing_progress: None,
        renders_game: true,
        hide_hand: false,
        custom_emote: None,
        remote_emotes,
        remote_hats: Vec::new(),
    };
    super::super::bodies::sample(
        Some(stream),
        &input,
        (
            super::super::bodies::PoseView {
                partial_tick: 0.5,
                first_person: true,
                java_mode,
                combat_visuals: Default::default(),
                sampling_camera: None,
                aspect: 1.0,
            },
            None,
        ),
        artwork,
        super::super::bodies::BodyCaches {
            poses: &mut Default::default(),
            java_poses: &mut Default::default(),
            native_pose: &mut Default::default(),
            skin_rigs: &mut Default::default(),
            equipment: None,
            scratch: &mut Default::default(),
        },
        |_, _, _, _| false,
        &mut Vec::new(),
    )
}

#[test]
fn shared_hats_follow_remote_heads_share_textures_and_reject_stale_spawns() {
    let ((mut stream, _, artwork, local_equipment), _) =
        super::native_tests::fixture_with_rotation(true);
    spawn_remotes(&mut stream);
    let selections: Vec<_> = [2, 3]
        .map(|runtime_id| super::super::RemoteHat {
            runtime_id,
            spawn_revision: stream.authority().actor(runtime_id).unwrap().spawn_revision,
            hat: render_model::hats::fixture::rigid(),
        })
        .into();
    let mut hats = super::super::hats::HatState::default();
    let mut scene = render_scene::ActorRenderScene::default();
    for selected in [
        selections.clone(),
        vec![
            super::super::RemoteHat {
                spawn_revision: selections[0].spawn_revision + 1,
                ..selections[0]
            },
            selections[1],
        ],
        Vec::new(),
    ] {
        let bodies = sample_shared_bodies(&stream, &artwork, &local_equipment, false, Vec::new());
        let mut batch =
            crate::presentation::actors::select_actor_presentations(1, false, None, bodies.remotes);
        let body_count = batch.submissions.len();
        let textures = batch.skin_layers.len();
        hats.advance(&[], 0.0, 1.0 / 60.0);
        hats.attach(
            &mut batch,
            Some(&stream),
            &selected,
            &mut scene,
            0.5,
            Default::default(),
        );
        let expected = selected
            .iter()
            .filter(|hat| {
                stream
                    .authority()
                    .actor(hat.runtime_id)
                    .unwrap()
                    .spawn_revision
                    == hat.spawn_revision
            })
            .count();
        assert_eq!(batch.submissions.len(), body_count + expected);
        assert_eq!(
            batch.skin_layers.len(),
            textures + usize::from(expected > 0)
        );
        for attachment in &batch.submissions[body_count..] {
            let id = attachment.input.identity.runtime_id;
            let head = stream
                .authority()
                .actor_rig(id)
                .unwrap()
                .bone_names
                .iter()
                .position(|name| name.eq_ignore_ascii_case("head"))
                .unwrap();
            let body = batch.submissions[..body_count]
                .iter()
                .find(|body| body.input.identity.runtime_id == id)
                .unwrap();
            assert_eq!(
                attachment.input.current_bones[0],
                body.input.current_bones[head]
            );
            assert_eq!(
                attachment.input.previous_bones[0],
                body.input.previous_bones[head]
            );
            assert_eq!(
                attachment.input.rig,
                render_model::hats::rig(render_model::hats::fixture::rigid())
                    .geometry
                    .id
            );
        }
    }
}

#[test]
fn shared_hats_render_multiple_catalog_slots_on_one_actor() {
    let ((mut stream, _, artwork, local_equipment), _) =
        super::native_tests::fixture_with_rotation(true);
    spawn_remotes(&mut stream);
    let items = [
        render_model::hats::fixture::bunny(),
        render_model::hats::fixture::dragon_wings(),
        render_model::hats::fixture::shoulder_parrot(),
    ];
    let selections: Vec<_> = items
        .into_iter()
        .map(|hat| super::super::RemoteHat {
            runtime_id: 2,
            spawn_revision: stream.authority().actor(2).unwrap().spawn_revision,
            hat,
        })
        .collect();
    let bodies = sample_shared_bodies(&stream, &artwork, &local_equipment, false, Vec::new());
    let mut batch =
        crate::presentation::actors::select_actor_presentations(1, false, None, bodies.remotes);
    let before = batch.submissions.len();
    let mut state = super::super::hats::HatState::default();
    state.advance(&[], 0.0, 0.0);
    let mut scene = render_scene::ActorRenderScene::default();
    state.attach(
        &mut batch,
        Some(&stream),
        &selections,
        &mut scene,
        0.5,
        Default::default(),
    );
    assert_eq!(batch.submissions.len(), before + items.len());
    let layers: std::collections::HashSet<_> = batch.submissions[before..]
        .iter()
        .map(|draw| draw.input.identity.layer)
        .collect();
    assert_eq!(layers.len(), items.len());
    for item in items {
        assert!(
            batch.submissions[before..]
                .iter()
                .any(|draw| draw.input.rig == render_model::hats::rig(item).geometry.id)
        );
    }
    let initial: std::collections::HashMap<_, _> = batch.submissions[before..]
        .iter()
        .map(|draw| (draw.input.rig, draw.input.current_bones.clone()))
        .collect();
    let bodies = sample_shared_bodies(&stream, &artwork, &local_equipment, false, Vec::new());
    let mut next =
        crate::presentation::actors::select_actor_presentations(1, false, None, bodies.remotes);
    let bodies = next.submissions.len();
    state.advance(&[], 0.0, 0.7);
    state.attach(
        &mut next,
        Some(&stream),
        &selections,
        &mut scene,
        0.5,
        Default::default(),
    );
    for item in items {
        let model = render_model::hats::rig(item);
        let draw = next.submissions[bodies..]
            .iter()
            .find(|draw| draw.input.rig == model.geometry.id)
            .unwrap();
        if ["back", "pet"].contains(&item.descriptor().slot.as_str()) {
            assert_ne!(
                initial[&model.geometry.id], draw.input.current_bones,
                "a stationary remote player's cosmetic animations keep advancing"
            );
        } else {
            assert_eq!(
                initial[&model.geometry.id], draw.input.current_bones,
                "animated cosmetics cannot move rigid headwear"
            );
        }
    }
}

#[test]
fn emissive_cosmetics_draw_their_world_raster_fullbright_and_place_emitters() {
    let ((mut stream, _, artwork, local_equipment), _) =
        super::native_tests::fixture_with_rotation(true);
    spawn_remotes(&mut stream);
    let glowing = render_model::hats::fixture::glowing_crown();
    let plain = render_model::hats::fixture::crown();
    let spawn_revision = stream.authority().actor(2).unwrap().spawn_revision;
    let selections: Vec<_> = [glowing, plain]
        .map(|hat| super::super::RemoteHat {
            runtime_id: 2,
            spawn_revision,
            hat,
        })
        .into();
    let bodies = sample_shared_bodies(&stream, &artwork, &local_equipment, false, Vec::new());
    let mut batch =
        crate::presentation::actors::select_actor_presentations(1, false, None, bodies.remotes);
    let before = batch.submissions.len();
    let mut state = super::super::hats::HatState::default();
    state.advance(&[], 0.0, 0.0);
    let mut scene = render_scene::ActorRenderScene::default();
    state.attach(
        &mut batch,
        Some(&stream),
        &selections,
        &mut scene,
        0.5,
        Default::default(),
    );
    let body = batch
        .submissions
        .iter()
        .find(|body| body.input.identity.runtime_id == 2)
        .unwrap()
        .clone();
    let rig = render_model::hats::rig(glowing);
    let lit_index = before
        + batch.submissions[before..]
            .iter()
            .position(|draw| draw.input.rig == rig.geometry.id)
            .unwrap();
    let lit = batch.submissions[lit_index].clone();
    let material = lit.material.state.unwrap();
    assert!(material.emissive && material.alpha_test && material.depth_write);
    assert!(!material.blend);
    assert_eq!(lit.material.kind, body.material.kind);
    assert!(lit.material.allows_overlay(), "hurt tint still applies");
    assert_eq!(
        batch.skin_layers[lit.texture_layer as usize].pixels(),
        rig.world_pixels().pixels()
    );
    let unlit = batch.submissions[before..]
        .iter()
        .find(|draw| draw.input.rig == render_model::hats::rig(plain).geometry.id)
        .unwrap();
    assert_eq!(unlit.material, body.material);
    assert_eq!(
        batch.skin_layers[unlit.texture_layer as usize].pixels(),
        render_model::hats::rig(plain).pixels.pixels()
    );

    let mut placements = Vec::new();
    state.place_emitters(&batch, 0.25, &mut placements);
    assert_eq!(placements.len(), 1);
    let placement = &placements[0];
    assert_eq!(
        placement.key,
        super::super::CosmeticEmitterKey {
            runtime_id: 2,
            spawn_revision,
            item: glowing,
            emitter: 0,
        }
    );
    assert_eq!(placement.identifier, rig.emitters()[0].identifier);
    let expected = rig
        .emitter_transform(
            0,
            &lit.input.previous_bones,
            &lit.input.current_bones,
            lit.world_from_actor,
            0.25,
        )
        .unwrap();
    assert_eq!(placement.transform, expected);
    let head = lit.input.current_bones[0].translation_scale;
    let pivot = lit
        .world_from_actor
        .map(|row| row[0] * head[0] + row[1] * head[1] + row[2] * head[2] + row[3]);
    assert!(
        placement.transform.position[1] > pivot[1] + 0.5,
        "the emitter rises above the posed head pivot"
    );

    batch.submissions[lit_index].route = render_model::ActorRigRoute::NoDraw;
    state.place_emitters(&batch, 0.25, &mut placements);
    assert!(placements.is_empty(), "hidden cosmetics stop emitting");
    state.attach(
        &mut batch,
        None,
        &selections,
        &mut scene,
        0.5,
        Default::default(),
    );
    state.place_emitters(&batch, 0.25, &mut placements);
    assert!(placements.is_empty(), "leaving the world clears placements");
}
