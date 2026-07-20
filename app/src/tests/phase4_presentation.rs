use std::sync::Arc;

use assets::EntityRigFallback;
use bevy::math::{Mat4, Quat, Vec3};
use client_world::{
    ActorLifetimeId, ActorPose, ActorRigSnapshot, ActorRigTextureSnapshot, ActorSnapshot,
    BoneTransform, EntityRigId, PlayerProfile,
};
use protocol::{ActorGameMode, ActorKind, ActorMetadataValue, PlayerSkin, StandardSkin};
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
    ActorRigPresentation, actor_rig_presentation, local_actor_presentation_for_visibility,
    local_diagnostic_presentation, select_actor_presentations, select_actor_presentations_for_view,
    update_actor_rig_scene,
};
use crate::runtime::network::{authoritative_local_actor_eye, publish_local_actor_visibility};

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
        previous,
        current,
        completed_tick: 11,
        reset_generation: 5,
        fallback: EntityRigFallback::GeometryOnly,
        texture: None,
    }
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
    assert_eq!((batch.atlas.width, batch.atlas.height), (32, 64));
    assert_eq!(batch.atlas.rgba8.as_ref(), pixels.as_ref());
    assert_eq!(
        batch.submissions[0].texture_region,
        [0.5 / 32.0, 0.5 / 64.0, 31.0 / 32.0, 63.0 / 64.0]
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
    assert_eq!(batch.atlas.rgba8.len(), STANDARD_SKIN_BYTES);
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

    let batch = select_actor_presentations_for_view(7, true, Some(local), [], Some(view));
    let mut scene = ActorRenderScene::default();
    let frame = update_actor_rig_scene(&mut scene, 0.5, batch);

    assert_eq!(frame.rig.instances.len(), 1);
    assert_eq!(frame.rig.manifest[0].identity.runtime_id, 7);
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
            assert_eq!(frame.skins_rgba8.len(), STANDARD_SKIN_BYTES);
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
