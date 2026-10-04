use super::*;
use std::num::NonZeroU64;

use crate::camera::{CameraSettingsAuthority, perspective_pose};
use crate::local_player::{
    CameraPose, InteractionOriginSnapshot, LocalAvatarPresentation, LocalAvatarVisibilityCarrier,
    LocalPlayerFrameCarrier, LocalPlayerFrameReset, LocalPlayerFrameSample, LocalViewPose,
};
use crate::movement::{
    MovementOutboxReconciliation, MovementSource, PhysicsAuthorityFault,
    PhysicsAuthorityFaultRecord, PhysicsCorrectionOutcome, PhysicsTickEvidence,
    PhysicsTickEvidenceContext,
};
use crate::runtime::phase3_evidence::{
    MAX_PHASE3_EVENT_RECORDS, MAX_PHASE3_FAULT_RECORDS, MAX_PHASE3_FRAME_RECORDS,
    Phase3EvidenceEmitter, Phase3EvidenceEventKind, Phase3EvidenceFrame, Phase3EvidenceIdentity,
    validate_phase3_build_source,
};
use crate::semantic_controls::{
    SemanticInputAuthorityFrame, SemanticInputRuntime, SemanticTouchTargets,
};
use bevy::math::Mat4;
use client_ui::ui_runtime::UiRuntime;
use render::{ActorCullView, ActorRenderScene, ActorRenderSource, MAX_RENDERED_PLAYERS};
use semantic_input::{
    Action, ControlSettings, ControllerFrame, DeviceFrame, InputContext, KeyboardMouseFrame,
    ReleaseReason, TouchContact,
};

fn frozen_collision_identity() -> sim::WorldCollisionIdentity {
    sim::WorldCollisionIdentity::new(
        sim::CollisionRegistryIdentity {
            protocol: 1001,
            id_space: sim::CollisionIdSpace::Sequential,
            preg_sha256: [0x3a; 32],
        },
        [
            world::ChunkCollisionRevision {
                chunk: world::ChunkKey::new(0, -2, 7),
                revision: 19,
            },
            world::ChunkCollisionRevision {
                chunk: world::ChunkKey::new(0, -1, 7),
                revision: 23,
            },
        ],
    )
    .unwrap()
}

fn frozen_local_player_sample_for(
    perspective: semantic_input::PerspectiveMode,
) -> LocalPlayerFrameSample {
    let eye = Vec3::new(8.0, 72.62, -4.0);
    let rotation = Quat::from_euler(bevy::math::EulerRot::YXZ, 0.8, -0.25, 0.0);
    LocalPlayerFrameSample {
        session_generation: 7,
        actor_session_id: 3,
        fifo_sequence: 41,
        physics_tick: 900,
        perspective,
        world_collision_identity: frozen_collision_identity(),
        pose: perspective_pose(eye, rotation, perspective),
        eye,
        feet: eye - Vec3::Y * protocol::PLAYER_NETWORK_OFFSET,
        rotation,
    }
}

fn frozen_local_player_sample() -> LocalPlayerFrameSample {
    frozen_local_player_sample_for(semantic_input::PerspectiveMode::ThirdPersonBack)
}

include!("camera/correction_and_evidence.rs");
include!("camera/presentation_and_input.rs");
include!("camera/crouch.rs");

#[test]
fn review_overflowing_quaternion_norm_is_rejected_without_publication() {
    let mut carrier = LocalPlayerFrameCarrier::default();
    let original = frozen_local_player_sample();
    carrier.publish(original.clone()).unwrap();
    let before = carrier.clone();
    let invalid = Quat::from_xyzw(f32::MAX, f32::MAX, f32::MAX, f32::MAX);
    let mut sample = original;
    sample.rotation = invalid;
    assert!(carrier.publish(sample).is_err());
    assert_eq!(carrier, before);
    let mut pose = LocalViewPose::default();
    let before = pose;
    pose.set_rotation(invalid);
    assert_eq!(pose, before);
}
