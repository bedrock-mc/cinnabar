use std::sync::Arc;

use assets::EntityRigFallback;
use bevy::math::{Mat4, Vec3};
use client_world::{
    ActorLifetimeId, ActorPose, ActorRigSnapshot, ActorSnapshot, BoneTransform, EntityRigId,
    PlayerProfile,
};
use protocol::{ActorGameMode, ActorKind, PlayerSkin, PlayerSkinUnavailable, StandardSkin};
use render::{
    ActorCullView, ActorRenderIdentity, ActorRenderScene, ActorRigRenderInput, ActorRigRoute,
    ActorRigSubmission, ActorSkinPixels, EntityRigId as RenderEntityRigId, MAX_RENDERED_PLAYERS,
    RenderBoneTransform, STANDARD_SKIN_BYTES, default_actor_skin_rgba8,
};
use semantic_input::PerspectiveMode;

use crate::presentation::actors::{
    ActorRigPresentation, actor_rig_presentation, select_actor_presentations,
    select_actor_presentations_for_view,
};

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

fn actor(runtime_id: u64, spawn_revision: u64, movement_revision: u64) -> ActorSnapshot {
    ActorSnapshot {
        unique_id: runtime_id as i64,
        runtime_id,
        spawn_revision,
        movement_revision,
        kind: ActorKind::Player {
            uuid: [runtime_id as u8; 16],
            username: "player".into(),
        },
        game_mode: Some(ActorGameMode::Survival),
        resolved_game_mode: Some(ActorGameMode::Survival),
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

fn profile_with_skin(unique_id: i64, skin: PlayerSkin) -> PlayerProfile {
    PlayerProfile {
        unique_id,
        username: "player".into(),
        verified: true,
        skin,
    }
}

fn render_owned(runtime_id: u64, spawn_revision: u64, skin: u8) -> ActorRigPresentation {
    ActorRigPresentation::from_render_submission(
        ActorRigSubmission {
            input: ActorRigRenderInput {
                identity: ActorRenderIdentity {
                    session_id: 7,
                    dimension: 0,
                    runtime_id,
                    spawn_revision,
                    ingress_sequence: spawn_revision,
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
            route: ActorRigRoute::Compiled,
        },
        ActorSkinPixels {
            width: 64,
            height: 64,
            rgba8: vec![skin; STANDARD_SKIN_BYTES].into(),
        },
    )
    .expect("finite render-owned local presentation")
}

fn diagnostic_render_owned(runtime_id: u64, spawn_revision: u64, skin: u8) -> ActorRigPresentation {
    let mut presentation = render_owned(runtime_id, spawn_revision, skin);
    let bones: Arc<[RenderBoneTransform]> = vec![render_bone(); 6].into();
    presentation.submission.input.previous_bones = Arc::clone(&bones);
    presentation.submission.input.current_bones = bones;
    presentation.submission.route = ActorRigRoute::Diagnostic;
    presentation
}

#[test]
fn actor_snapshot_conversion_preserves_identity_pose_and_model_space_units() {
    let actor = actor(42, 3, 0);
    let previous = [model_bone([16.0, 0.0, 0.0])];
    let current = [model_bone([32.0, 0.0, 0.0])];
    let rig = ActorRigSnapshot {
        actor: ActorLifetimeId {
            session_id: 7,
            dimension: 0,
            runtime_id: 42,
            spawn_revision: 3,
        },
        rig: EntityRigId(9),
        previous: &previous,
        current: &current,
        completed_tick: 11,
        reset_generation: 5,
        fallback: EntityRigFallback::GeometryOnly,
    };

    let converted = actor_rig_presentation(&rig, &actor, Some(&profile(42, 7)), 0.5)
        .expect("exact player rig converts");

    assert_eq!(converted.submission.input.identity.session_id, 7);
    assert_eq!(converted.submission.input.identity.runtime_id, 42);
    assert_eq!(converted.submission.input.identity.spawn_revision, 3);
    assert_eq!(converted.submission.input.identity.movement_revision, 0);
    assert_eq!(converted.submission.input.identity.pose_generation, 11);
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
    assert_eq!(converted.submission.world_from_actor[1][3], 64.0);
    assert_eq!(converted.submission.route, ActorRigRoute::StaticFallback);
}

#[test]
fn conversion_rejects_nonfinite_bones_and_mismatched_lifetimes() {
    let actor = actor(42, 3, 1);
    let previous = [model_bone([0.0; 3])];
    let mut invalid = model_bone([0.0; 3]);
    invalid.rotation[0] = f32::NAN;
    let current = [invalid];
    let mut rig = ActorRigSnapshot {
        actor: ActorLifetimeId {
            session_id: 7,
            dimension: 0,
            runtime_id: 42,
            spawn_revision: 3,
        },
        rig: EntityRigId(9),
        previous: &previous,
        current: &current,
        completed_tick: 11,
        reset_generation: 5,
        fallback: EntityRigFallback::GeometryOnly,
    };

    assert!(actor_rig_presentation(&rig, &actor, Some(&profile(42, 7)), 0.5).is_none());
    rig.current = &previous;
    rig.actor.spawn_revision = 4;
    assert!(actor_rig_presentation(&rig, &actor, Some(&profile(42, 7)), 0.5).is_none());
}

#[test]
fn native_entity_without_reviewed_material_family_is_attributed_no_draw() {
    let mut actor = actor(55, 3, 1);
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:bee".into(),
    };
    let bones = [model_bone([0.0; 3])];
    let rig = ActorRigSnapshot {
        actor: ActorLifetimeId {
            session_id: 7,
            dimension: 0,
            runtime_id: 55,
            spawn_revision: 3,
        },
        rig: EntityRigId(2),
        previous: &bones,
        current: &bones,
        completed_tick: 11,
        reset_generation: 5,
        fallback: EntityRigFallback::GeometryOnly,
    };

    let presentation = actor_rig_presentation(&rig, &actor, None, 1.0)
        .expect("unsupported native material remains attributable");
    assert_eq!(presentation.submission.route, ActorRigRoute::NoDraw);
    assert!(presentation.skin_rgba8.is_none());
}

#[test]
fn every_missing_or_unusable_player_skin_family_uses_the_local_default() {
    let actor = actor(42, 3, 1);
    let bones = [model_bone([0.0; 3])];
    let rig = ActorRigSnapshot {
        actor: ActorLifetimeId {
            session_id: 7,
            dimension: 0,
            runtime_id: 42,
            spawn_revision: 3,
        },
        rig: EntityRigId(9),
        previous: &bones,
        current: &bones,
        completed_tick: 11,
        reset_generation: 5,
        fallback: EntityRigFallback::GeometryOnly,
    };
    let unavailable = [
        PlayerSkinUnavailable::UnsupportedPersona,
        PlayerSkinUnavailable::InvalidDimensions,
        PlayerSkinUnavailable::InvalidByteLength,
        PlayerSkinUnavailable::RetainedBudgetExceeded,
    ];
    let mut cases = vec![None, Some(profile(99, 7))];
    cases.extend(
        unavailable
            .into_iter()
            .map(|reason| Some(profile_with_skin(42, PlayerSkin::Unavailable(reason)))),
    );
    cases.extend([
        Some(profile_with_skin(
            42,
            PlayerSkin::Standard(StandardSkin {
                width: 64,
                height: 32,
                rgba8: vec![7; 64 * 32 * 4].into(),
            }),
        )),
        Some(profile_with_skin(
            42,
            PlayerSkin::Standard(StandardSkin {
                width: 32,
                height: 32,
                rgba8: vec![7; 32 * 32 * 4].into(),
            }),
        )),
        Some(profile_with_skin(
            42,
            PlayerSkin::Standard(StandardSkin {
                width: 64,
                height: 64,
                rgba8: vec![7; STANDARD_SKIN_BYTES - 1].into(),
            }),
        )),
    ]);
    let expected = default_actor_skin_rgba8();

    for profile in &cases {
        let presentation = actor_rig_presentation(&rig, &actor, profile.as_ref(), 1.0)
            .expect("valid player pose remains renderable with a local fallback skin");
        assert_eq!(presentation.submission.route, ActorRigRoute::StaticFallback);
        let skin = presentation
            .skin_rgba8
            .expect("every player fallback owns a texture layer");
        assert_eq!(skin.as_ref(), expected.as_ref());
        assert!(Arc::ptr_eq(&skin, &expected));
    }
}

#[test]
fn local_world_avatar_is_zero_one_one_and_owns_its_runtime_id() {
    let local = render_owned(7, 5, 19);
    let duplicate_remote = render_owned(7, 4, 23);
    let ordinary_remote = render_owned(8, 1, 29);

    for (perspective, expected_local_count) in [
        (PerspectiveMode::FirstPerson, 0),
        (PerspectiveMode::ThirdPersonBack, 1),
        (PerspectiveMode::ThirdPersonFront, 1),
    ] {
        let batch = select_actor_presentations(
            perspective,
            7,
            Some(local.clone()),
            [duplicate_remote.clone(), ordinary_remote.clone()],
        );
        assert_eq!(
            batch
                .submissions
                .iter()
                .filter(|actor| actor.input.identity.runtime_id == 7)
                .count(),
            expected_local_count
        );
        assert_eq!(
            batch
                .submissions
                .iter()
                .filter(|actor| actor.input.identity.runtime_id == 8)
                .count(),
            1
        );
    }
}

#[test]
fn visible_local_reserves_capacity_and_identical_skin_families_are_shared() {
    let local = render_owned(10_000, 1, 31);
    let remotes = (1..=MAX_RENDERED_PLAYERS as u64)
        .rev()
        .map(|runtime_id| render_owned(runtime_id, 1, 31))
        .collect::<Vec<_>>();

    let batch = select_actor_presentations(
        PerspectiveMode::ThirdPersonBack,
        10_000,
        Some(local),
        remotes,
    );

    assert_eq!(batch.submissions.len(), MAX_RENDERED_PLAYERS);
    assert_eq!(
        batch
            .submissions
            .iter()
            .filter(|actor| actor.input.identity.runtime_id == 10_000)
            .count(),
        1
    );
    assert_eq!(batch.skins_rgba8.len(), STANDARD_SKIN_BYTES);
    assert!(
        batch
            .submissions
            .iter()
            .all(|actor| actor.texture_layer == 0)
    );
}

#[test]
fn culled_and_no_draw_low_ids_cannot_starve_a_later_visible_player() {
    let view = ActorCullView {
        clip_from_world: Mat4::from_scale(Vec3::splat(0.001)),
        camera_position: Vec3::new(0.0, 65.0, 0.0),
        max_distance: 192.0,
    };
    let mut remotes = (1..=MAX_RENDERED_PLAYERS as u64)
        .map(|runtime_id| {
            let mut actor = diagnostic_render_owned(runtime_id, 1, 41);
            if runtime_id % 2 == 0 {
                actor.submission.route = ActorRigRoute::NoDraw;
            } else {
                actor.submission.world_from_actor[0][3] = 500.0;
            }
            actor
        })
        .collect::<Vec<_>>();
    remotes.push(diagnostic_render_owned(129, 1, 41));

    let batch = select_actor_presentations_for_view(
        PerspectiveMode::FirstPerson,
        10_000,
        None,
        remotes,
        Some(view),
    );
    assert!(
        batch
            .submissions
            .iter()
            .any(|actor| actor.input.identity.runtime_id == 129),
        "the visible player after the runtime-ID prefix must reach render admission",
    );

    let mut scene = ActorRenderScene::default();
    let frame = scene.update_rigs(0.5, Some(view), batch.submissions, batch.skins_rgba8);
    assert_eq!(frame.rig.instances.len(), 1);
    assert_eq!(frame.rig.manifest[0].identity.runtime_id, 129);
    assert_eq!(frame.rig.rejects.no_draw, 64);
}

#[test]
fn world_transform_is_finite_before_render_submission() {
    let actor = actor(42, 3, 1);
    let bones = [model_bone([0.0; 3])];
    let rig = ActorRigSnapshot {
        actor: ActorLifetimeId {
            session_id: 7,
            dimension: 0,
            runtime_id: 42,
            spawn_revision: 3,
        },
        rig: EntityRigId(9),
        previous: &bones,
        current: &bones,
        completed_tick: 11,
        reset_generation: 5,
        fallback: EntityRigFallback::GeometryOnly,
    };
    let mut actor = actor;
    actor.position = [Vec3::NAN.x, 64.0, 0.0];

    assert!(actor_rig_presentation(&rig, &actor, Some(&profile(42, 7)), 0.5).is_none());
}
