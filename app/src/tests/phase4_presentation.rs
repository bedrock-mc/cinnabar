use std::sync::Arc;

use assets::EntityRigFallback;
use bevy::math::{Mat4, Quat, Vec3};
use client_world::{
    ActorLifetimeId, ActorPose, ActorRigSnapshot, ActorRigTextureSnapshot, ActorSnapshot,
    BoneTransform, CommittedUiEvent, EntityRigId, LocalPlayerRigSnapshot, PlayerProfile,
};
use protocol::{
    ActorGameMode, ActorKind, ActorMetadataValue, PlayerGameMode, PlayerSkin, StandardSkin,
};
use render::{
    ActorCullView, ActorRenderIdentity, ActorRenderScene, ActorRigRenderInput, ActorRigRoute,
    ActorRigSubmission, EntityRigId as RenderEntityRigId, MAX_RENDERED_PLAYERS,
    RenderBoneTransform, STANDARD_SKIN_BYTES,
};
use semantic_input::PerspectiveMode;

use crate::local_player::{
    LocalAvatarPresentation, LocalAvatarVisibilityCarrier, LocalPlayerFrameCarrier,
    LocalPlayerFrameSample,
};
use crate::movement::{MovementSource, PhysicsAuthorityGate};
use crate::presentation::actors::{
    ActorRigPresentation, LocalPlayerPresentationAuthority, actor_rig_presentation,
    local_actor_presentation_for_visibility, local_diagnostic_presentation,
    local_player_rig_presentation, select_actor_presentations, select_actor_presentations_for_view,
    update_actor_rig_scene,
};
use crate::runtime::network::{authoritative_local_actor_eye, publish_local_actor_visibility};
use crate::runtime::world::apply_committed_ui_event;
use crate::ui_runtime::UiRuntime;

mod custom_geometry;

fn model_bone(translation: [f32; 3]) -> BoneTransform {
    BoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [translation[0], translation[1], translation[2], 1.0],
    }
}

fn render_bone() -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [0.0, 0.0, 0.0, 1.0],
    }
}

fn actor(runtime_id: u64, movement_revision: u64) -> ActorSnapshot {
    ActorSnapshot {
        unique_id: runtime_id as i64,
        runtime_id,
        spawn_revision: 3,
        movement_revision,
        kind: ActorKind::Player {
            uuid: [runtime_id as u8; 16],
            username: "player".into(),
        },
        game_mode: None,
        resolved_game_mode: None,
        game_mode_tick: None,
        position: [4.0, 64.0, -2.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 90.0,
        head_yaw: 90.0,
        previous_pose: ActorPose {
            position: [2.0, 64.0, -2.0],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
        },
        received_pose: ActorPose {
            position: [4.0, 64.0, -2.0],
            pitch: 0.0,
            yaw: 90.0,
            head_yaw: 90.0,
        },
        interpolation_ticks_remaining: 0,
        body_yaw: 90.0,
        on_ground: Some(true),
        teleported: false,
        player_mode: None,
        source_tick: Some(41),
        metadata: Default::default(),
        attributes: Default::default(),
        int_properties: Default::default(),
        float_properties: Default::default(),
    }
}

fn profile(runtime_id: u64, value: u8) -> PlayerProfile {
    PlayerProfile {
        unique_id: runtime_id as i64,
        username: "player".into(),
        verified: true,
        skin: PlayerSkin::Standard(StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![value; STANDARD_SKIN_BYTES].into(),
            geometry: protocol::PlayerSkinGeometry::Wide,
        }),
    }
}

fn rig<'a>(
    runtime_id: u64,
    previous: &'a [BoneTransform],
    current: &'a [BoneTransform],
) -> ActorRigSnapshot<'a> {
    ActorRigSnapshot {
        actor: ActorLifetimeId {
            session_id: 7,
            dimension: 0,
            runtime_id,
            spawn_revision: 3,
        },
        rig: EntityRigId(9),
        geometry_identifier: "geometry.humanoid.custom",
        geometry_sha256: [0; 32],
        previous,
        current,
        completed_tick: 11,
        reset_generation: 5,
        fallback: EntityRigFallback::GeometryOnly,
        texture: None,
    }
}

fn local_rig<'a>(bones: &'a [BoneTransform]) -> LocalPlayerRigSnapshot<'a> {
    LocalPlayerRigSnapshot {
        rig: EntityRigId(17),
        geometry_identifier: "geometry.humanoid.custom",
        geometry_sha256: [0; 32],
        previous: bones,
        current: bones,
        completed_tick: 11,
        reset_generation: 5,
        fallback: EntityRigFallback::GeometryOnly,
    }
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at the pinned official resource pack"]
fn pinned_carrier_reaches_local_and_remote_render_manifests_with_exact_player_authority() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .expect("set PINNED_VANILLA_PACK to the pinned official resource pack");
    let compiled = asset_compiler::compile_entity_assets(
        std::path::Path::new(&pack),
        include_bytes!("../../../assets/vanilla-source.json"),
    )
    .unwrap();
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    let entity_assets = Arc::new(assets::RuntimeEntityAssets::decode(&bytes).unwrap());
    let bootstrap = protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    };
    let mut stream = client_world::WorldStream::new_with_asset_sets(
        bootstrap,
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::clone(&entity_assets),
        [0.0, 64.0, 0.0],
        None,
    );
    stream.set_local_player_game_mode_authority(protocol::LocalPlayerGameModeAuthority::new(
        -9,
        ActorGameMode::Survival,
        ActorGameMode::Survival,
    ));
    let skin = |geometry, byte| {
        PlayerSkin::Standard(StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![byte; 64 * 64 * 4].into(),
            geometry,
        })
    };
    let legacy_skin = |geometry, byte| {
        PlayerSkin::Standard(StandardSkin {
            width: 64,
            height: 32,
            rgba8: [byte, byte, byte, 255].repeat(64 * 32).into(),
            geometry,
        })
    };
    stream
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Add {
                        uuid: [9; 16],
                        unique_id: -9,
                        username: "self".into(),
                        verified: true,
                        skin: legacy_skin(protocol::PlayerSkinGeometry::Wide, 0x5a),
                    }]),
                },
            )),
        )
        .unwrap();
    stream.advance_local_player_animation(client_world::LocalPlayerAnimationTickInput {
        tick: 1,
        velocity: [0.25, 0.0, 0.0],
        on_ground: true,
        body_yaw: 30.0,
        head_yaw: 30.0,
        pitch: 0.0,
    });
    stream.advance_local_player_animation(client_world::LocalPlayerAnimationTickInput {
        tick: 2,
        velocity: [0.25, 0.0, 0.0],
        on_ground: true,
        body_yaw: 30.0,
        head_yaw: 45.0,
        pitch: -10.0,
    });
    let wide_snapshot = stream.local_player_rig().expect("wide local rig");
    let wide_geometry_sha256 = wide_snapshot.geometry_sha256;
    assert_ne!(
        wide_snapshot.previous, wide_snapshot.current,
        "authoritative movement/view ticks must change the real compiled pose"
    );
    let local = local_player_rig_presentation(
        &wide_snapshot,
        stream.local_player_profile().expect("exact local profile"),
        LocalPlayerPresentationAuthority {
            actor_session_id: 7,
            dimension: 0,
            runtime_id: 42,
            pose_generation: 2,
            position: [0.0, 64.0, 0.0],
            yaw_degrees: 30.0,
        },
    )
    .expect("carrier-backed local presentation");
    assert_eq!(local.submission.route, ActorRigRoute::StaticFallback);
    let wide_rig = local.submission.input.rig;
    let batch = select_actor_presentations_for_view(42, true, Some(local), [], |_| true);
    let mut scene = ActorRenderScene::with_runtime_entity_assets(&entity_assets).unwrap();
    let frame = update_actor_rig_scene(&mut scene, 0.5, batch);
    assert_eq!(frame.rig.manifest.len(), 1);
    assert_eq!(frame.rig.manifest[0].identity.runtime_id, 42);
    assert_eq!(frame.rig.manifest[0].route, ActorRigRoute::StaticFallback);
    assert_eq!(
        (frame.texture_atlas_width, frame.texture_atlas_height),
        (66, 66)
    );
    let atlas_pixel = |x: usize, y: usize| {
        let offset = ((y + 1) * frame.texture_atlas_width as usize + x + 1) * 4;
        &frame.skins_rgba8[offset..offset + 4]
    };
    assert_eq!(atlas_pixel(0, 0), &[0x5a, 0x5a, 0x5a, 255]);
    assert_eq!(
        atlas_pixel(20, 48),
        &[0x5a, 0x5a, 0x5a, 255],
        "WorldStream legacy authority reaches the canonical local presentation"
    );
    assert_eq!(atlas_pixel(0, 32), &[0; 4]);

    stream
        .submit(
            2,
            protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Add {
                        uuid: [9; 16],
                        unique_id: -9,
                        username: "self".into(),
                        verified: true,
                        skin: skin(protocol::PlayerSkinGeometry::Slim, 0x6b),
                    }]),
                },
            )),
        )
        .unwrap();
    stream.advance_local_player_animation(client_world::LocalPlayerAnimationTickInput {
        tick: 3,
        velocity: [0.4, 0.0, 0.0],
        on_ground: true,
        body_yaw: 60.0,
        head_yaw: 70.0,
        pitch: 5.0,
    });
    let slim = stream.local_player_rig().expect("slim local rig");
    assert_eq!(slim.geometry_identifier, "geometry.humanoid.customSlim");
    assert_ne!(RenderEntityRigId(slim.rig.0), wide_rig);

    stream
        .submit(
            3,
            protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Add {
                        uuid: [9; 16],
                        unique_id: -9,
                        username: "self".into(),
                        verified: true,
                        skin: skin(
                            protocol::PlayerSkinGeometry::Custom {
                                identifier: "geometry.humanoid.custom".into(),
                                data_sha256: wide_geometry_sha256,
                            },
                            0x7c,
                        ),
                    }]),
                },
            )),
        )
        .unwrap();
    assert!(
        stream.local_player_rig().is_some(),
        "packet geometry exactly matching the pinned fingerprint is renderable"
    );
    let exact_custom = local_player_rig_presentation(
        &stream.local_player_rig().unwrap(),
        stream.local_player_profile().unwrap(),
        LocalPlayerPresentationAuthority {
            actor_session_id: 7,
            dimension: 0,
            runtime_id: 42,
            pose_generation: 3,
            position: [0.0, 64.0, 0.0],
            yaw_degrees: 60.0,
        },
    )
    .expect("exact pinned custom geometry reaches presentation");
    let exact_custom_batch =
        select_actor_presentations_for_view(42, true, Some(exact_custom), [], |_| true);
    let mut exact_custom_scene =
        ActorRenderScene::with_runtime_entity_assets(&entity_assets).unwrap();
    let exact_custom_frame =
        update_actor_rig_scene(&mut exact_custom_scene, 0.5, exact_custom_batch);
    assert_eq!(exact_custom_frame.rig.manifest.len(), 1);
    assert!(
        exact_custom_frame
            .skins_rgba8
            .iter()
            .all(|byte| *byte == 0x7c)
    );

    let spawn = |runtime_id: u64, unique_id: i64, uuid: [u8; 16]| {
        protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id,
            runtime_id,
            kind: ActorKind::Player {
                uuid,
                username: format!("remote-{runtime_id}").into(),
            },
            game_mode: Some(ActorGameMode::Survival),
            position: [2.0, 64.0, 2.0],
            velocity: [0.2, 0.0, 0.0],
            pitch: 0.0,
            yaw: 20.0,
            head_yaw: 25.0,
            body_yaw: 20.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
        }))
    };
    let roster = |uuid, unique_id, geometry, byte| {
        protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
            protocol::PlayerListUpdateEvent {
                entries: Arc::from([protocol::PlayerListEntry::Add {
                    uuid,
                    unique_id,
                    username: format!("remote-{unique_id}").into(),
                    verified: true,
                    skin: skin(geometry, byte),
                }]),
            },
        ))
    };

    stream
        .submit(
            4,
            roster([51; 16], 51, protocol::PlayerSkinGeometry::Wide, 0x51),
        )
        .unwrap();
    stream.submit(5, spawn(51, 51, [51; 16])).unwrap();
    assert_eq!(
        stream.actor_rig(51).unwrap().geometry_identifier,
        "geometry.humanoid.custom"
    );

    stream.submit(6, spawn(52, 52, [52; 16])).unwrap();
    assert!(
        stream.actor_rig(52).is_none(),
        "spawn before roster is NoDraw"
    );
    stream
        .submit(
            7,
            roster([52; 16], 52, protocol::PlayerSkinGeometry::Slim, 0x52),
        )
        .unwrap();
    assert_eq!(
        stream.actor_rig(52).unwrap().geometry_identifier,
        "geometry.humanoid.customSlim"
    );
    stream.advance_actor_interpolation_ticks(1);
    let remote = actor_rig_presentation(
        &stream.actor_rig(52).unwrap(),
        stream.actor(52).unwrap(),
        stream.actor_player_profile(52),
        0.5,
    )
    .expect("remote exact profile presentation");
    assert_eq!(remote.submission.route, ActorRigRoute::StaticFallback);
    let remote_batch = select_actor_presentations_for_view(42, false, None, [remote], |_| true);
    let mut remote_scene = ActorRenderScene::with_runtime_entity_assets(&entity_assets).unwrap();
    let remote_frame = update_actor_rig_scene(&mut remote_scene, 0.5, remote_batch);
    assert_eq!(remote_frame.rig.manifest.len(), 1);
    assert_eq!(remote_frame.rig.manifest[0].identity.runtime_id, 52);
    assert_eq!(
        remote_frame.rig.manifest[0].route,
        ActorRigRoute::StaticFallback
    );
    assert!(remote_frame.skins_rgba8.iter().all(|byte| *byte == 0x52));

    stream
        .submit(
            8,
            roster([51; 16], 51, protocol::PlayerSkinGeometry::Slim, 0x61),
        )
        .unwrap();
    assert_eq!(
        stream.actor_rig(51).unwrap().geometry_identifier,
        "geometry.humanoid.customSlim"
    );
    stream
        .submit(
            9,
            protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Remove { uuid: [51; 16] }]),
                },
            )),
        )
        .unwrap();
    assert!(stream.actor_rig(51).is_none(), "roster removal removes rig");
    stream
        .submit(
            10,
            roster(
                [52; 16],
                52,
                protocol::PlayerSkinGeometry::Custom {
                    identifier: "geometry.humanoid.customSlim".into(),
                    data_sha256: [0; 32],
                },
                0x62,
            ),
        )
        .unwrap();
    assert!(
        stream.actor_rig(52).is_none(),
        "remote custom geometry is NoDraw"
    );
    stream
        .submit(
            11,
            roster([52; 16], 52, protocol::PlayerSkinGeometry::Slim, 0x72),
        )
        .unwrap();
    assert!(stream.actor_rig(52).is_some());
    stream
        .submit(
            12,
            roster([53; 16], 52, protocol::PlayerSkinGeometry::Slim, 0x73),
        )
        .unwrap();
    assert!(
        stream.actor_rig(52).is_none(),
        "ambiguous unique-id authority is NoDraw"
    );
    stream
        .submit(
            13,
            protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Remove { uuid: [53; 16] }]),
                },
            )),
        )
        .unwrap();
    assert!(stream.actor_rig(52).is_some());
    stream
        .submit(
            14,
            protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Add {
                        uuid: [52; 16],
                        unique_id: 52,
                        username: "remote-52".into(),
                        verified: true,
                        skin: protocol::PlayerSkin::Unavailable(
                            protocol::PlayerSkinUnavailable::UnsupportedPersona,
                        ),
                    }]),
                },
            )),
        )
        .unwrap();
    assert!(
        stream.actor_rig(52).is_none(),
        "unavailable appearance authority is NoDraw"
    );
    stream
        .submit(
            15,
            protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Add {
                        uuid: [9; 16],
                        unique_id: -9,
                        username: "self".into(),
                        verified: true,
                        skin: skin(
                            protocol::PlayerSkinGeometry::Custom {
                                identifier: "geometry.humanoid.custom".into(),
                                data_sha256: [0; 32],
                            },
                            0x7d,
                        ),
                    }]),
                },
            )),
        )
        .unwrap();
    assert!(
        stream.local_player_rig().is_none(),
        "mismatched packet geometry fingerprint remains NoDraw"
    );
}

#[test]
fn local_player_uses_compiled_geometry_and_authoritative_skin_without_defaulting() {
    let bones = [model_bone([0.0, 24.0, 0.0]), model_bone([0.0, 12.0, 0.0])];
    let mut profile = profile(9, 37);
    profile.unique_id = -9;
    let presentation = local_player_rig_presentation(
        &local_rig(&bones),
        &profile,
        LocalPlayerPresentationAuthority {
            actor_session_id: 7,
            dimension: 0,
            runtime_id: 42,
            pose_generation: 12,
            position: [1.0, 64.0, 2.0],
            yaw_degrees: 90.0,
        },
    )
    .expect("exact local geometry and skin authority");

    assert_eq!(presentation.submission.route, ActorRigRoute::StaticFallback);
    assert_eq!(presentation.submission.input.rig, RenderEntityRigId(17));
    assert!(
        presentation
            .texture
            .unwrap()
            .rgba8
            .iter()
            .all(|byte| *byte == 37)
    );

    let mut unavailable = profile;
    unavailable.skin = PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions);
    assert!(
        local_player_rig_presentation(
            &local_rig(&bones),
            &unavailable,
            LocalPlayerPresentationAuthority {
                actor_session_id: 7,
                dimension: 0,
                runtime_id: 42,
                pose_generation: 12,
                position: [1.0, 64.0, 2.0],
                yaw_degrees: 90.0,
            },
        )
        .is_none(),
        "malformed skin authority must not synthesize Steve"
    );
}

#[test]
fn local_and_remote_presentations_reject_skin_rig_geometry_mismatches() {
    let bones = [model_bone([0.0, 24.0, 0.0])];
    let mut slim_profile = profile(9, 37);
    slim_profile.unique_id = -9;
    let PlayerSkin::Standard(skin) = &mut slim_profile.skin else {
        unreachable!();
    };
    skin.geometry = protocol::PlayerSkinGeometry::Slim;
    assert!(
        local_player_rig_presentation(
            &local_rig(&bones),
            &slim_profile,
            LocalPlayerPresentationAuthority {
                actor_session_id: 7,
                dimension: 0,
                runtime_id: 42,
                pose_generation: 12,
                position: [1.0, 64.0, 2.0],
                yaw_degrees: 0.0,
            },
        )
        .is_none()
    );
    let remote_actor = actor(9, 11);
    slim_profile.unique_id = 9;
    let presentation = actor_rig_presentation(
        &rig(9, &bones, &bones),
        &remote_actor,
        Some(&slim_profile),
        0.5,
    )
    .unwrap();
    assert_eq!(presentation.submission.route, ActorRigRoute::NoDraw);
}

fn render_owned(runtime_id: u64, skin: u8) -> ActorRigPresentation {
    ActorRigPresentation {
        submission: ActorRigSubmission {
            input: ActorRigRenderInput {
                identity: ActorRenderIdentity {
                    session_id: 7,
                    dimension: 0,
                    runtime_id,
                    spawn_revision: 3,
                    ingress_sequence: 3,
                    source_tick: None,
                    movement_revision: 0,
                    pose_generation: 11,
                },
                rig: RenderEntityRigId(3),
                previous_bones: Arc::from([render_bone()]),
                current_bones: Arc::from([render_bone()]),
                completed_tick: 11,
                reset_generation: 5,
            },
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 64.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            texture_layer: u32::MAX,
            texture_region: [0.0, 0.0, 1.0, 1.0],
            route: ActorRigRoute::Compiled,
        },
        texture: Some(render::ActorTexturePixels {
            width: 64,
            height: 64,
            rgba8: vec![skin; STANDARD_SKIN_BYTES].into(),
        }),
    }
}

#[test]
fn actor_snapshot_conversion_preserves_identity_pose_and_model_space_units() {
    let actor = actor(42, 0);
    let previous = [model_bone([16.0, 0.0, 0.0])];
    let current = [model_bone([32.0, 0.0, 0.0])];

    let converted = actor_rig_presentation(
        &rig(42, &previous, &current),
        &actor,
        Some(&profile(42, 7)),
        0.5,
    )
    .expect("exact player rig converts before its first movement packet");

    assert_eq!(converted.submission.input.identity.session_id, 7);
    assert_eq!(converted.submission.input.identity.movement_revision, 0);
    assert_eq!(converted.submission.input.rig, RenderEntityRigId(9));
    assert_eq!(
        converted.submission.input.previous_bones[0].translation_scale[0],
        1.0
    );
    assert_eq!(
        converted.submission.input.current_bones[0].translation_scale[0],
        2.0
    );
    assert_eq!(converted.submission.world_from_actor[0][3], 3.0);
    assert_eq!(converted.submission.route, ActorRigRoute::StaticFallback);
    assert!(
        converted
            .texture
            .as_ref()
            .is_some_and(|texture| texture.rgba8.iter().all(|byte| *byte == 7)),
        "the selected non-default roster skin survives conversion",
    );

    let absent = actor_rig_presentation(&rig(42, &previous, &current), &actor, None, 0.5)
        .expect("exact lifetime remains attributable without skin authority");
    assert_eq!(absent.submission.route, ActorRigRoute::NoDraw);
    assert!(absent.texture.is_none());
}

#[test]
fn production_rig_conversion_rejects_spectators_before_selection_capacity() {
    let bones = [model_bone([0.0; 3])];
    let mut spectator = actor(1, 1);
    spectator.resolved_game_mode = Some(ActorGameMode::Spectator);
    let rejected = actor_rig_presentation(&rig(1, &bones, &bones), &spectator, None, 0.5)
        .expect("an exact ineligible lifetime publishes an explicit no-draw route");
    assert_eq!(rejected.submission.route, ActorRigRoute::NoDraw);

    let mut invisible = actor(2, 1);
    invisible
        .metadata
        .insert(0, ActorMetadataValue::Flags(1 << 5));
    let invisible = actor_rig_presentation(&rig(2, &bones, &bones), &invisible, None, 0.5)
        .expect("an exact invisible lifetime publishes an explicit no-draw route");
    assert_eq!(invisible.submission.route, ActorRigRoute::NoDraw);

    let remotes = [rejected, invisible]
        .into_iter()
        .chain((3..=MAX_RENDERED_PLAYERS as u64 + 2).map(|id| render_owned(id, 31)))
        .collect::<Vec<_>>();
    let batch = select_actor_presentations(999, false, None, remotes);

    assert_eq!(batch.submissions.len(), MAX_RENDERED_PLAYERS);
    assert!(
        batch
            .submissions
            .iter()
            .all(|entry| entry.route != ActorRigRoute::NoDraw)
    );
    assert!(
        batch
            .submissions
            .iter()
            .any(|entry| { entry.input.identity.runtime_id == MAX_RENDERED_PLAYERS as u64 + 2 })
    );
}

#[test]
fn compiled_non_player_texture_reaches_the_production_atlas_without_player_fallback() {
    let bones = [model_bone([0.0; 3])];
    let mut allay = actor(44, 1);
    allay.kind = ActorKind::Entity {
        identifier: "minecraft:allay".into(),
    };
    let pixels: Arc<[u8]> = vec![73; 32 * 64 * 4].into();
    let mut allay_rig = rig(44, &bones, &bones);
    allay_rig.texture = Some(ActorRigTextureSnapshot {
        width: 32,
        height: 64,
        rgba8: &pixels,
    });
    let presentation = actor_rig_presentation(&allay_rig, &allay, None, 0.5)
        .expect("exact allay lifetime converts");
    assert_eq!(presentation.submission.route, ActorRigRoute::StaticFallback);
    assert_eq!(
        presentation
            .texture
            .as_ref()
            .map(|texture| (texture.width, texture.height)),
        Some((32, 64))
    );

    let batch = select_actor_presentations(999, false, None, [presentation]);
    assert_eq!((batch.atlas.width, batch.atlas.height), (34, 66));
    assert!(batch.atlas.rgba8.iter().all(|channel| *channel == 73));
    assert_eq!(
        batch.submissions[0].texture_region,
        [1.0 / 34.0, 1.0 / 66.0, 32.0 / 34.0, 64.0 / 66.0]
    );

    let missing = actor_rig_presentation(&rig(44, &bones, &bones), &allay, None, 0.5)
        .expect("exact unsupported lifetime remains attributable");
    assert_eq!(missing.submission.route, ActorRigRoute::NoDraw);
    assert!(missing.texture.is_none());
}

#[test]
fn local_spectator_canonical_route_cannot_fall_back_to_synthetic_avatar() {
    let bones = [model_bone([0.0; 3])];
    let mut spectator = actor(7, 1);
    spectator.resolved_game_mode = Some(ActorGameMode::Spectator);
    let canonical = actor_rig_presentation(&rig(7, &bones, &bones), &spectator, None, 0.5)
        .expect("exact spectator identity remains attributable");
    let diagnostic = local_diagnostic_presentation(7, 0, 7, 5, [0.0, 64.0, 0.0], 0.0, 0.0)
        .expect("finite synthetic avatar");
    let selected =
        local_actor_presentation_for_visibility(7, 7, true, Some(canonical), Some(diagnostic));
    let batch = select_actor_presentations(7, true, selected, []);

    assert!(batch.submissions.is_empty());
}

#[test]
fn start_game_only_local_spectator_cannot_use_the_synthetic_f5_fallback() {
    let diagnostic = local_diagnostic_presentation(7, 0, 7, 5, [0.0, 64.0, 0.0], 0.0, 0.0)
        .expect("finite synthetic avatar");

    let selected = local_actor_presentation_for_visibility(7, 7, false, None, Some(diagnostic));
    let batch = select_actor_presentations(7, true, selected, []);

    assert!(batch.submissions.is_empty());
}

#[test]
fn committed_local_mode_authority_updates_the_live_ui_runtime() {
    let mut ui_runtime = UiRuntime::new(7);
    ui_runtime.publish_player_game_mode(PlayerGameMode::Survival);

    apply_committed_ui_event(
        &mut ui_runtime,
        7,
        100,
        CommittedUiEvent::LocalGameMode {
            sequence: 3,
            game_mode: PlayerGameMode::Spectator,
        },
    )
    .expect("ordered local mode authority reaches the UI runtime");

    assert_eq!(
        ui_runtime.player_game_mode(),
        Some(PlayerGameMode::Spectator)
    );
    assert!(!ui_runtime.survival_stats_visible());
}

#[test]
fn conversion_rejects_nonfinite_bones_and_mismatched_lifetimes() {
    let actor = actor(42, 1);
    let previous = [model_bone([0.0; 3])];
    let mut invalid = model_bone([0.0; 3]);
    invalid.rotation[0] = f32::NAN;
    let current = [invalid];
    assert!(
        actor_rig_presentation(
            &rig(42, &previous, &current),
            &actor,
            Some(&profile(42, 7)),
            0.5
        )
        .is_none()
    );

    let mut mismatched = rig(42, &previous, &previous);
    mismatched.actor.spawn_revision = 4;
    assert!(actor_rig_presentation(&mismatched, &actor, Some(&profile(42, 7)), 0.5).is_none());
}

#[test]
fn visible_local_reserves_one_slot_and_removes_its_remote_duplicate() {
    let local = local_diagnostic_presentation(7, -1, 7, 5, [0.0, 64.0, 0.0], 0.0, 0.0)
        .expect("finite local carrier converts");
    assert_eq!(local.submission.input.identity.dimension, -1);
    let remotes = (1..=MAX_RENDERED_PLAYERS as u64 + 1)
        .rev()
        .map(|runtime_id| render_owned(runtime_id, 31))
        .collect::<Vec<_>>();

    let hidden = select_actor_presentations(7, false, Some(local.clone()), remotes.clone());
    assert_eq!(hidden.submissions.len(), MAX_RENDERED_PLAYERS);
    assert_eq!(
        hidden
            .submissions
            .iter()
            .filter(|entry| entry.input.identity.runtime_id == 7)
            .count(),
        0
    );

    let visible = select_actor_presentations(7, true, Some(local), remotes);
    assert_eq!(visible.submissions.len(), MAX_RENDERED_PLAYERS);
    assert_eq!(
        visible
            .submissions
            .iter()
            .filter(|entry| entry.input.identity.runtime_id == 7)
            .count(),
        1
    );
}

#[test]
fn local_visibility_identity_gates_all_perspective_routes() {
    let canonical = render_owned(7, 31);
    let mismatched_visibility =
        local_diagnostic_presentation(7, 0, 8, 5, [100.0, 64.0, 0.0], 0.0, 0.0)
            .expect("finite mismatched visibility converts");
    let local = local_actor_presentation_for_visibility(
        7,
        8,
        true,
        Some(canonical.clone()),
        Some(mismatched_visibility),
    );
    let batch = select_actor_presentations(7, true, local, [render_owned(7, 31)]);
    assert!(batch.submissions.is_empty());

    let matching_visibility =
        local_diagnostic_presentation(7, 0, 7, 5, [100.0, 64.0, 0.0], 0.0, 0.0)
            .expect("finite matching visibility converts");
    for (perspective, expected_local_draws) in [
        (PerspectiveMode::FirstPerson, 0),
        (PerspectiveMode::ThirdPersonBack, 1),
        (PerspectiveMode::ThirdPersonFront, 1),
    ] {
        let local = local_actor_presentation_for_visibility(
            7,
            7,
            true,
            Some(canonical.clone()),
            Some(matching_visibility.clone()),
        );
        let batch = select_actor_presentations(
            7,
            perspective != PerspectiveMode::FirstPerson,
            local,
            [render_owned(7, 31)],
        );
        assert_eq!(
            batch
                .submissions
                .iter()
                .filter(|entry| entry.input.identity.runtime_id == 7)
                .count(),
            expected_local_draws,
            "unexpected local draw count for {perspective:?}",
        );
    }
}

#[test]
fn identical_skin_families_share_one_bounded_texture_layer() {
    let batch =
        select_actor_presentations(99, false, None, [render_owned(1, 31), render_owned(2, 31)]);
    assert_eq!(batch.atlas.rgba8.len(), 66 * 66 * 4);
    assert!(
        batch
            .submissions
            .iter()
            .all(|entry| entry.texture_layer == 0)
    );
}

#[test]
fn visible_local_is_reserved_even_when_the_world_frustum_excludes_its_body() {
    let mut local = local_diagnostic_presentation(7, 0, 7, 5, [0.0, 64.0, 0.0], 0.0, 0.0)
        .expect("finite local carrier converts");
    local.submission.world_from_actor[0][3] = 500.0;
    let view = ActorCullView {
        clip_from_world: Mat4::from_scale(Vec3::splat(0.001)),
        camera_position: Vec3::new(0.0, 65.0, 0.0),
        max_distance: 192.0,
    };

    let batch = select_actor_presentations_for_view(7, true, Some(local), [], |_| {
        let _ = view;
        false
    });
    let mut scene = ActorRenderScene::default();
    let frame = update_actor_rig_scene(&mut scene, 0.5, batch);

    assert_eq!(frame.rig.instances.len(), 1);
    assert_eq!(frame.rig.manifest[0].identity.runtime_id, 7);
}

#[test]
fn missing_geometry_cannot_starve_a_valid_actor_before_the_app_capacity_gate() {
    let scene = ActorRenderScene::default();
    let missing = (0..MAX_RENDERED_PLAYERS as u64).map(|runtime_id| render_owned(runtime_id, 31));
    let mut valid = render_owned(MAX_RENDERED_PLAYERS as u64 + 1, 47);
    valid.submission.route = ActorRigRoute::Diagnostic;

    let batch = select_actor_presentations_for_view(
        999,
        false,
        None,
        missing.chain([valid]),
        |submission| scene.rig_submission_is_visible(submission, None),
    );

    assert_eq!(batch.submissions.len(), 1);
    assert_eq!(
        batch.submissions[0].input.identity.runtime_id,
        MAX_RENDERED_PLAYERS as u64 + 1
    );
}

#[test]
fn third_person_local_fallback_reaches_the_render_manifest_without_a_physics_frame() {
    assert_eq!(
        PhysicsAuthorityGate::ProductionDisabled.authorize(false, true),
        Ok(MovementSource::FreeCamera)
    );
    let local_frame = LocalPlayerFrameCarrier::default();
    assert!(local_frame.snapshot().is_none());

    let mut no_identity = LocalAvatarPresentation::default();
    no_identity.begin_session(7, 0);
    let mut visibility = LocalAvatarVisibilityCarrier::default();
    no_identity.publish_view_visibility(
        PerspectiveMode::ThirdPersonBack,
        Vec3::new(3.0, 65.62, -2.0),
        Quat::IDENTITY,
        &mut visibility,
    );
    assert!(visibility.snapshot().is_none());

    let mut avatar = LocalAvatarPresentation::default();
    avatar.begin_session(7, 42);
    for (perspective, expected_draws) in [
        (PerspectiveMode::FirstPerson, 0),
        (PerspectiveMode::ThirdPersonBack, 1),
        (PerspectiveMode::ThirdPersonFront, 1),
    ] {
        avatar.publish_view_visibility(
            perspective,
            Vec3::new(3.0, 65.62, -2.0),
            Quat::IDENTITY,
            &mut visibility,
        );
        let snapshot = visibility
            .snapshot()
            .copied()
            .expect("valid session view publishes without Physics authority");
        assert_eq!(snapshot.visible(), expected_draws != 0);

        let mut position = snapshot.eye();
        position.y -= crate::local_player::LOCAL_AVATAR_EYE_HEIGHT_BLOCKS;
        let local = local_diagnostic_presentation(
            9,
            0,
            snapshot.runtime_id(),
            snapshot.pose_generation(),
            position.to_array(),
            0.0,
            0.0,
        )
        .expect("view-backed local visibility converts to a diagnostic rig");
        let batch = select_actor_presentations(42, snapshot.visible(), Some(local), []);
        let mut scene = ActorRenderScene::default();
        let frame = update_actor_rig_scene(&mut scene, 0.5, batch);

        assert_eq!(frame.rig.instances.len(), expected_draws);
        assert_eq!(frame.rig.manifest.len(), expected_draws);
        if expected_draws != 0 {
            assert_eq!(frame.rig.manifest[0].identity.runtime_id, 42);
            assert_eq!(frame.rig.manifest[0].route, ActorRigRoute::Diagnostic);
            assert_eq!(frame.skins_rgba8.len(), 66 * 66 * 4);
        }
    }

    avatar.publish_view_visibility(
        PerspectiveMode::ThirdPersonBack,
        Vec3::NAN,
        Quat::IDENTITY,
        &mut visibility,
    );
    assert!(visibility.snapshot().is_none());
}

#[test]
fn f5_local_avatar_uses_authoritative_subject_when_view_eye_is_boomed() {
    let subject_eye = Vec3::new(64.0, 70.62, -512.0);
    let subject_rotation = Quat::from_rotation_y(90.0_f32.to_radians());
    let stale_eye = subject_eye + Vec3::Z * 8.0;
    let mut stale_frame = LocalPlayerFrameCarrier::default();
    let collision_identity = sim::WorldCollisionIdentity::new(
        sim::CollisionRegistryIdentity {
            protocol: 1001,
            id_space: sim::CollisionIdSpace::Sequential,
            preg_sha256: [0x5a; 32],
        },
        [world::ChunkCollisionRevision {
            chunk: world::ChunkKey::new(0, 4, -32),
            revision: 9,
        }],
    )
    .unwrap();
    let stale_sample = LocalPlayerFrameSample {
        session_generation: 7,
        fifo_sequence: 41,
        physics_tick: 900,
        perspective: PerspectiveMode::ThirdPersonBack,
        world_collision_identity: collision_identity,
        pose: crate::camera::perspective_pose(
            stale_eye,
            subject_rotation,
            PerspectiveMode::ThirdPersonBack,
        ),
        eye: stale_eye,
        rotation: subject_rotation,
    };
    stale_frame.publish(stale_sample).unwrap();

    let mut avatar = LocalAvatarPresentation::default();
    avatar.begin_session(7, 42);
    let mut visibility = LocalAvatarVisibilityCarrier::default();
    for perspective in [
        PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonFront,
    ] {
        let camera = crate::camera::perspective_pose(subject_eye, subject_rotation, perspective);
        assert_ne!(camera.translation, subject_eye);
        let authoritative_eye =
            authoritative_local_actor_eye(Some(subject_eye.to_array()), Some(stale_eye.to_array()));
        publish_local_actor_visibility(
            &avatar,
            perspective,
            authoritative_eye,
            subject_rotation,
            &mut visibility,
        );
        let snapshot = visibility.snapshot().copied().unwrap();
        assert_eq!(snapshot.eye(), subject_eye);
        assert!(snapshot.visible());

        let mut feet = snapshot.eye();
        feet.y -= crate::local_player::LOCAL_AVATAR_EYE_HEIGHT_BLOCKS;
        let local = local_diagnostic_presentation(
            7,
            0,
            snapshot.runtime_id(),
            snapshot.pose_generation(),
            feet.to_array(),
            90.0,
            0.0,
        )
        .unwrap();
        let world_from_actor = local.submission.world_from_actor;
        let body_center = Vec3::new(
            world_from_actor[0][3],
            world_from_actor[1][3] + 1.0,
            world_from_actor[2][3],
        );
        let clip_from_world =
            Mat4::perspective_infinite_reverse_rh(70.0_f32.to_radians(), 16.0 / 9.0, 0.1)
                * camera.to_matrix().inverse();
        let projected_center = clip_from_world * body_center.extend(1.0);
        assert!(projected_center.w > 0.0);
        assert!((projected_center.x / projected_center.w).abs() < 1.0e-5);
    }

    publish_local_actor_visibility(
        &avatar,
        PerspectiveMode::FirstPerson,
        Some(subject_eye),
        subject_rotation,
        &mut visibility,
    );
    assert!(!visibility.snapshot().unwrap().visible());

    assert_eq!(
        authoritative_local_actor_eye(None, Some(subject_eye.to_array())),
        Some(subject_eye)
    );
    assert_eq!(authoritative_local_actor_eye(None, None), None);
}
