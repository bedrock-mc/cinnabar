//! Perspective placement and allocation-free boom collision.

use bevy::{log::debug, prelude::*};
use render_api::CAMERA_NEAR_PLANE_BLOCKS;
use semantic_input::PerspectiveMode;
use sim::{CollisionWorld, LenientSkipCounts, Vec3 as SimVec3};

use super::{CameraRig, THIRD_PERSON_COLLISION_RADIUS_BLOCKS, THIRD_PERSON_RADIUS_BLOCKS};

/// Computes preset placement before the collision stage shortens its boom.
#[must_use]
pub fn perspective_pose(
    subject_translation: Vec3,
    subject_rotation: Quat,
    perspective: PerspectiveMode,
) -> Transform {
    let forward = subject_rotation * Vec3::NEG_Z;
    match perspective {
        PerspectiveMode::FirstPerson => Transform {
            translation: subject_translation,
            rotation: subject_rotation,
            ..default()
        },
        PerspectiveMode::ThirdPersonBack => {
            let translation = subject_translation - forward * THIRD_PERSON_RADIUS_BLOCKS;
            Transform {
                translation,
                rotation: subject_rotation,
                ..default()
            }
        }
        PerspectiveMode::ThirdPersonFront => {
            // Vanilla reverse orbit retains the full player look vector;
            // looking at the player keeps global Y as up.
            let translation = subject_translation + forward * THIRD_PERSON_RADIUS_BLOCKS;
            Transform::from_translation(translation).looking_at(subject_translation, Vec3::Y)
        }
    }
}

/// Traces eight corner rays along the third-person boom, skipping unreadable cells.
/// Invalid queries retain the full preset reach.
#[must_use]
pub fn collision_safe_perspective_pose(
    subject_translation: Vec3,
    subject_rotation: Quat,
    perspective: PerspectiveMode,
    world: &impl CollisionWorld,
) -> Transform {
    let pose = perspective_pose(subject_translation, subject_rotation, perspective);
    if perspective == PerspectiveMode::FirstPerson {
        return pose;
    }
    sweep_boom(subject_translation, pose, world)
}

/// The rig's boom from the eye, keeping the eye's look direction.
#[must_use]
pub fn rig_pose(subject_translation: Vec3, subject_rotation: Quat, rig: CameraRig) -> Transform {
    Transform {
        translation: subject_translation + subject_rotation * rig.offset,
        rotation: subject_rotation,
        ..default()
    }
}

/// Shortens a rig boom against collision exactly like the third-person boom.
#[must_use]
pub fn collision_safe_rig_pose(
    subject_translation: Vec3,
    subject_rotation: Quat,
    rig: CameraRig,
    world: &impl CollisionWorld,
) -> Transform {
    sweep_boom(
        subject_translation,
        rig_pose(subject_translation, subject_rotation, rig),
        world,
    )
}

/// Clips eight corner rays along the boom, walking only the cells each ray crosses.
pub(super) fn sweep_boom(
    subject_translation: Vec3,
    mut pose: Transform,
    world: &impl CollisionWorld,
) -> Transform {
    let delta = pose.translation - subject_translation;
    let origin = SimVec3::new(
        f64::from(subject_translation.x),
        f64::from(subject_translation.y),
        f64::from(subject_translation.z),
    );
    let sweep = SimVec3::new(f64::from(delta.x), f64::from(delta.y), f64::from(delta.z));
    let radius = f64::from(THIRD_PERSON_COLLISION_RADIUS_BLOCKS);
    let distance = f64::from(delta.length());
    let near_clip = f64::from(CAMERA_NEAR_PLANE_BLOCKS);
    let mut safe_distance = distance;
    let mut skipped = LenientSkipCounts::default();
    for corner in 0..8 {
        let offset = SimVec3::new(
            if corner & 1 == 0 { -radius } else { radius },
            if corner & 2 == 0 { -radius } else { radius },
            if corner & 4 == 0 { -radius } else { radius },
        );
        let Ok((entry, corner_skipped)) = world.camera_segment_entry(origin + offset, sweep) else {
            continue;
        };
        skipped.unknown_runtime_id = skipped
            .unknown_runtime_id
            .saturating_add(corner_skipped.unknown_runtime_id);
        skipped.unloaded_chunk = skipped
            .unloaded_chunk
            .saturating_add(corner_skipped.unloaded_chunk);
        if let Some(entry) = entry {
            let hit = offset + sweep * entry;
            safe_distance =
                safe_distance.min((hit.length_squared().sqrt() - near_clip).max(near_clip));
        }
    }
    if safe_distance < distance {
        pose.translation =
            subject_translation + delta.normalize_or_zero() * safe_distance.max(0.25) as f32;
    }
    record_boom_telemetry(pose.translation.distance(subject_translation), skipped);
    pose
}

/// Logs only changes to the resolved radius bucket or skip tally.
fn record_boom_telemetry(radius: f32, skipped: LenientSkipCounts) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static LAST_STATE: AtomicU32 = AtomicU32::new(u32::MAX);

    let bucket = (radius.clamp(0.0, THIRD_PERSON_RADIUS_BLOCKS) * 4.0).round() as u32;
    let unknown = skipped.unknown_runtime_id.min(0xFF);
    let unloaded = skipped.unloaded_chunk.min(0xFF);
    let state = bucket | (unknown << 16) | (unloaded << 24);
    if LAST_STATE.swap(state, Ordering::Relaxed) == state {
        return;
    }
    debug!(
        boom_radius = radius,
        skipped_unknown_runtime_id = skipped.unknown_runtime_id,
        skipped_unloaded_chunk = skipped.unloaded_chunk,
        "third-person camera boom resolved"
    );
}

/// Keeps the camera at the eye until authoritative collision data is available.
#[must_use]
pub fn unavailable_world_perspective_pose(
    subject_translation: Vec3,
    subject_rotation: Quat,
    _perspective: PerspectiveMode,
) -> Transform {
    Transform {
        translation: subject_translation,
        rotation: subject_rotation,
        ..default()
    }
}
