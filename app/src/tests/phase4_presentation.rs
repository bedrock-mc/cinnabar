use std::sync::Arc;

use assets::EntityRigFallback;
use bevy::math::{Mat4, Quat, Vec3};
use client_world::{
    ActorLifetimeId, ActorPose, ActorRigSnapshot, ActorSnapshot, BoneTransform, EntityRigId,
    PlayerProfile,
};
use protocol::{ActorKind, PlayerSkin, StandardSkin};
use render::{
    ActorCullView, ActorRenderIdentity, ActorRenderScene, ActorRigRenderInput, ActorRigRoute,
    ActorRigSubmission,
};
use render_model::{
    EntityRigId as RenderEntityRigId, MAX_RENDERED_PLAYERS, RenderBoneTransform,
    STANDARD_SKIN_BYTES,
};
use semantic_input::PerspectiveMode;

use crate::local_player::{
    LocalAvatarPresentation, LocalAvatarVisibilityCarrier, LocalPlayerFrameCarrier,
    LocalPlayerFrameSample,
};
use crate::movement::{MovementSource, PhysicsAuthorityGate};
use crate::presentation::actors::{
    ActorRigPresentation, actor_rig_presentation, entity_rig_presentation,
    local_actor_presentation_for_visibility, local_diagnostic_presentation,
    select_actor_presentations, select_actor_presentations_for_view, update_actor_rig_scene,
};
use crate::runtime::network::{authoritative_local_actor_eye, publish_local_actor_visibility};

fn model_bone(translation: [f32; 3]) -> BoneTransform {
    BoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [translation[0], translation[1], translation[2], 1.0],
        axis_scale: [1.0; 3],
    }
}

fn render_bone() -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [0.0, 0.0, 0.0, 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    }
}

fn actor(runtime_id: u64, movement_revision: u64) -> ActorSnapshot {
    let mut actor = super::actor_snapshot(protocol::ActorSpawnEvent {
        dimension: 0,
        unique_id: runtime_id as i64,
        runtime_id,
        kind: ActorKind::Player {
            uuid: [runtime_id as u8; 16],
            username: "player".into(),
        },
        position: [4.0, 64.0, -2.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 90.0,
        head_yaw: 90.0,
        body_yaw: 90.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    });
    actor.spawn_revision = 3;
    actor.movement_revision = movement_revision;
    actor.on_ground = Some(true);
    actor.source_tick = Some(41);
    actor.previous_pose = ActorPose {
        position: [2.0, 64.0, -2.0],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
    };
    actor
}

fn profile(runtime_id: u64, value: u8) -> PlayerProfile {
    PlayerProfile {
        unique_id: runtime_id as i64,
        username: "player".into(),
        verified: true,
        skin: PlayerSkin::Standard(StandardSkin {
            geometry: None,
            cape: None,
            width: render_model::STANDARD_SKIN_SIDE as u32,
            height: render_model::STANDARD_SKIN_SIDE as u32,
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
        rest: previous,
        rest_completed_tick: 11,
        rest_reset_generation: 5,
        completed_tick: 11,
        reset_generation: 5,
        fallback: EntityRigFallback::GeometryOnly,
        scale: 1.0,
        axis_scale: [1.0; 3],
        previous_body_yaw: 0.0,
        body_yaw: 0.0,
        render: &[],
        bone_names: &[],
        skin_geometry: None,
        skin_layers: &[],
        hand: Default::default(),
        item_animation: [client_world::ItemAnimationState::default(); 2],
        off_hand_animation: [client_world::ItemAnimationState::default(); 2],
        animation_variables: Default::default(),
        java: Default::default(),
        java_equipped: None,
    }
}

fn render_owned(runtime_id: u64, skin: u8) -> ActorRigPresentation {
    ActorRigPresentation {
        submission: ActorRigSubmission {
            material: Default::default(),
            culling_bounds: Default::default(),
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
                    layer: 0,
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
            route: ActorRigRoute::Compiled,
            tint: 0,
            uv_anim: render::IDENTITY_UV_ANIM,
            light: 0,
            overlay_rgba8: 0,
        },
        skin_rgba8: Some(vec![skin; STANDARD_SKIN_BYTES].into()),
        artwork: None,
        authored_scale: 1.0,
        world_yaw_degrees: 0.0,
        head_over_body: 0.0,
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
            .skin_rgba8
            .as_ref()
            .is_some_and(|skin| skin.iter().all(|byte| *byte == 7)),
        "the selected non-default roster skin survives conversion",
    );
}

#[test]
fn generic_actor_without_validated_artwork_remains_explicitly_no_draw() {
    let mut actor = actor(42, 0);
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:example".into(),
    };
    let bones = [model_bone([0.0; 3])];
    let presentation = entity_rig_presentation(
        &rig(42, &bones, &bones),
        &actor,
        &render::ActorArtworkPages::default(),
        0.5,
    )
    .unwrap();
    assert_eq!(presentation.submission.route, ActorRigRoute::NoDraw);
    assert!(presentation.artwork.is_none());
    assert!(presentation.skin_rgba8.is_none());
    let batch = select_actor_presentations(7, false, None, [presentation]);
    assert_eq!(batch.submissions.len(), 1);
    assert_eq!(batch.submissions[0].route, ActorRigRoute::NoDraw);
    assert!(batch.skin_layers.is_empty());
    assert!(batch.artwork.is_empty());
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
fn visible_local_removes_its_remote_duplicate_without_capping_other_actors() {
    let local = local_diagnostic_presentation(7, -1, 7, 5, [0.0, 64.0, 0.0], 0.0, 0.0)
        .expect("finite local carrier converts");
    assert_eq!(local.submission.input.identity.dimension, -1);
    let remotes = (1..=MAX_RENDERED_PLAYERS as u64 + 1)
        .rev()
        .map(|runtime_id| render_owned(runtime_id, 31))
        .collect::<Vec<_>>();
    let remote_count = remotes.len();

    let hidden = select_actor_presentations(7, false, Some(local.clone()), remotes.clone());
    assert_eq!(hidden.submissions.len(), remote_count - 1);
    assert_eq!(
        hidden
            .submissions
            .iter()
            .filter(|entry| entry.input.identity.runtime_id == 7)
            .count(),
        0
    );

    let visible = select_actor_presentations(7, true, Some(local), remotes);
    assert_eq!(visible.submissions.len(), remote_count);
    assert_eq!(
        visible
            .submissions
            .iter()
            .filter(|entry| entry.input.identity.runtime_id == 7)
            .count(),
        1
    );
    assert!(visible.submissions.iter().any(|entry| {
        entry.input.identity.runtime_id == MAX_RENDERED_PLAYERS as u64 + 1
    }));
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
        Some(canonical.clone()),
        Some(mismatched_visibility),
        0.0,
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
            Some(canonical.clone()),
            Some(matching_visibility.clone()),
            0.0,
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
    assert_eq!(batch.skin_layers.len(), 1);
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
        Vec3::new(3.0, 64.0, -2.0),
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
            Vec3::new(3.0, 64.0, -2.0),
            Quat::IDENTITY,
            &mut visibility,
        );
        let snapshot = visibility
            .snapshot()
            .copied()
            .expect("valid session view publishes without Physics authority");
        assert_eq!(snapshot.visible(), expected_draws != 0);

        let position = snapshot.feet();
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
            let skin = frame.player_skin(frame.rig.instances[0].texture_layer);
            assert_eq!(skin.map(|skin| skin.len()), Some(STANDARD_SKIN_BYTES));
        }
    }

    avatar.publish_view_visibility(
        PerspectiveMode::ThirdPersonBack,
        Vec3::NAN,
        Vec3::ZERO,
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
        actor_session_id: 3,
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
        feet: stale_eye - Vec3::Y * protocol::PLAYER_NETWORK_OFFSET,
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
            None,
            authoritative_eye,
            Some(subject_eye - Vec3::Y * protocol::PLAYER_NETWORK_OFFSET),
            subject_rotation,
            &mut visibility,
        );
        let snapshot = visibility.snapshot().copied().unwrap();
        assert_eq!(snapshot.eye(), subject_eye);
        assert!(snapshot.visible());

        let feet = snapshot.feet();
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
        None,
        Some(subject_eye),
        Some(subject_eye - Vec3::Y * protocol::PLAYER_NETWORK_OFFSET),
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

#[test]
fn local_canonical_body_lags_the_view_yaw_by_the_rigs_head_offset() {
    let mut canonical = render_owned(7, 31);
    canonical.head_over_body = 30.0;
    canonical.submission.world_from_actor =
        crate::presentation::actors::rig_world_from_actor([0.0; 3], 0.0, 1.0);
    let diagnostic = local_diagnostic_presentation(7, 0, 7, 5, [4.0, 64.0, 2.0], 90.0, 0.0)
        .expect("finite local carrier converts");
    let local =
        local_actor_presentation_for_visibility(7, 7, Some(canonical), Some(diagnostic), 90.0)
            .expect("canonical local rig is kept");
    assert_eq!(
        local.submission.world_from_actor,
        crate::presentation::actors::rig_world_from_actor([4.0, 64.0, 2.0], 60.0, 1.0)
    );
}

#[test]
fn projectile_animation_rotation_is_not_multiplied_by_mob_body_yaw() {
    for identifier in [
        "minecraft:arrow",
        "minecraft:ender_pearl",
        "minecraft:snowball",
    ] {
        let mut actor = actor(42, 1);
        actor.kind = ActorKind::Entity {
            identifier: identifier.into(),
        };
        for pitch in [-90.0_f32, -35.0, 0.0, 90.0] {
            let rotation = (Quat::from_rotation_y(73.0_f32.to_radians())
                * Quat::from_rotation_x(pitch.to_radians()))
            .to_array();
            let bones = [BoneTransform {
                rotation,
                ..model_bone([0.0; 3])
            }];
            for body_yaw in [-120.0, 0.0, 90.0] {
                let rig = ActorRigSnapshot {
                    previous_body_yaw: body_yaw,
                    body_yaw,
                    ..rig(42, &bones, &bones)
                };
                let presentation = entity_rig_presentation(
                    &rig,
                    &actor,
                    &render::ActorArtworkPages::default(),
                    1.0,
                )
                .unwrap();
                let rows = presentation.submission.world_from_actor;
                assert_eq!(presentation.world_yaw_degrees, 0.0, "{identifier}");
                assert!((rows[0][0] + 1.0).abs() < 1e-6, "{identifier}");
                assert!(rows[0][2].abs() < 1e-6, "{identifier}");
                assert!(rows[2][0].abs() < 1e-6, "{identifier}");
                assert!((rows[2][2] + 1.0).abs() < 1e-6, "{identifier}");
                assert_eq!(
                    presentation.submission.input.current_bones[0].rotation, rotation,
                    "{identifier} keeps its authored rotation at pitch {pitch}"
                );
                assert_eq!(
                    presentation.submission.input.previous_bones[0].rotation,
                    rotation
                );
            }
        }
    }
}

#[test]
fn authored_skin_bounds_reach_frustum_and_cave_admission() {
    let patch = r#"{"geometry":{"default":"geometry.capture_bounds"}}"#;
    let model = r#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.capture_bounds","texture_width":64,"texture_height":64,
            "visible_bounds_width":3,"visible_bounds_height":4,"visible_bounds_offset":[0,2,0]},
        "bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#;
    let geometry = Arc::new(assets::parse_skin_geometry(patch, model).unwrap().unwrap());
    let mut actor = actor(42, 41);
    actor.position = [2.0, 64.0, 0.0];
    actor.previous_pose.position = actor.position;
    let bones = [model_bone([0.0; 3])];
    let default = rig(42, &bones, &bones);
    let authored = ActorRigSnapshot {
        skin_geometry: Some(&geometry),
        ..default
    };
    let view = ActorCullView {
        clip_from_world: Mat4::from_translation(Vec3::new(0.0, -65.0, 0.0)),
        camera_position: Vec3::new(0.0, 65.0, 0.0),
        max_distance: 192.0,
    };
    assert!(!crate::presentation::actors::rig_may_be_visible(
        &default,
        &actor,
        1.0,
        Some(view),
        |_, _| false,
    ));
    assert!(crate::presentation::actors::rig_may_be_visible(
        &authored,
        &actor,
        1.0,
        Some(view),
        |low, high| {
            assert_eq!(low, [0.5, 64.0, -1.5]);
            assert_eq!(high, [3.5, 68.0, 1.5]);
            false
        },
    ));
    let body = actor_rig_presentation(&authored, &actor, Some(&profile(42, 255)), 1.0).unwrap();
    assert!(render::actor_rig_submission_is_visible(
        &body.submission,
        Some(view)
    ));
    assert_eq!(
        body.submission.culling_bounds,
        geometry.visible_bounds.unwrap()
    );
}
