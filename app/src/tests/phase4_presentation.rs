use std::sync::Arc;

use assets::EntityRigFallback;
use client_world::{
    ActorLifetimeId, ActorPose, ActorRigSnapshot, ActorSnapshot, BoneTransform, EntityRigId,
    PlayerProfile,
};
use protocol::{ActorKind, PlayerSkin, StandardSkin};
use render::{
    ActorRenderIdentity, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission,
    EntityRigId as RenderEntityRigId, MAX_RENDERED_PLAYERS, RenderBoneTransform,
    STANDARD_SKIN_BYTES,
};

use crate::presentation::actors::{
    ActorRigPresentation, actor_rig_presentation, local_diagnostic_presentation,
    select_actor_presentations,
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
            route: ActorRigRoute::Compiled,
        },
        skin_rgba8: Some(vec![skin; STANDARD_SKIN_BYTES].into()),
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
fn identical_skin_families_share_one_bounded_texture_layer() {
    let batch =
        select_actor_presentations(99, false, None, [render_owned(1, 31), render_owned(2, 31)]);
    assert_eq!(batch.skins_rgba8.len(), STANDARD_SKIN_BYTES);
    assert!(
        batch
            .submissions
            .iter()
            .all(|entry| entry.texture_layer == 0)
    );
}
