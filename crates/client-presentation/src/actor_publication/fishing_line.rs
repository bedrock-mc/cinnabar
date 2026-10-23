//! Fishing-line attachment to the rendered main-hand item.

use super::hand::HandSource;
use bevy::prelude::{Projection, Transform, Vec3};

#[cfg(test)]
mod tests;

/// The world-space point where a rendered fishing rod releases its line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FishingLineAnchor {
    pub(crate) runtime_id: u64,
    pub(crate) from: [f32; 3],
}

/// The vanilla fishing-rod sprite's tip is the outside corner of its end pixel at `(14, 1)`.
/// `held_sprite_vertices` maps that icon into the held-item frame, whose X axis is mirrored.
const FISHING_ROD_TIP: [f32; 3] = [-15.0 / 16.0, 15.0 / 16.0, 0.0];

/// Samples the held sprite tip through the submitted root-bone transform.
fn fishing_line_anchor_for_submission(
    submission: &render_model::ActorRigSubmission,
    partial_tick: f32,
) -> Option<[f32; 3]> {
    let previous = submission.input.previous_bones.first().copied()?;
    let current = submission.input.current_bones.first().copied()?;
    render_model::actor::sampled_bone_offset(
        FISHING_ROD_TIP,
        previous,
        current,
        submission.world_from_actor,
        partial_tick,
    )
    .map(|anchor| anchor.position)
}

/// Samples the same root bone used by the held-item layer so a fishing line stays attached while
/// the hand swings. A missing layer leaves the world-authority fallback untouched.
pub(super) fn fishing_line_anchors_for(
    batch: &crate::presentation::actors::ActorPresentationBatch,
    partial_tick: f32,
    owner_is_fishing_rod: impl Fn(u64) -> bool,
) -> Vec<FishingLineAnchor> {
    batch
        .submissions
        .iter()
        .filter(|submission| {
            submission.input.identity.layer == crate::presentation::equipment::LAYER_MAIN_HAND
                && owner_is_fishing_rod(submission.input.identity.runtime_id)
        })
        .filter_map(|submission| {
            let from = fishing_line_anchor_for_submission(submission, partial_tick)?;
            Some(FishingLineAnchor {
                runtime_id: submission.input.identity.runtime_id,
                from,
            })
        })
        .collect()
}

/// Reprojects a point from the hand pass into the world camera's view so a world-space rope keeps
/// the same screen position as the camera-local held item. The hand and world passes deliberately
/// use different FOVs, and directly applying the camera transform would therefore leave a gap.
fn hand_point_to_world_view(
    point: [f32; 3],
    world_projection: &Projection,
    aspect: f32,
    hand_fov: f32,
) -> Option<Vec3> {
    if !aspect.is_finite() || aspect <= 0.0 || !hand_fov.is_finite() || hand_fov <= 0.0 {
        return None;
    }
    let hand_projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        hand_fov,
        aspect,
        render_api::CAMERA_NEAR_PLANE_BLOCKS,
    );
    let hand_clip = hand_projection * Vec3::from_array(point).extend(1.0);
    if !hand_clip.is_finite() || hand_clip.w <= 0.0 {
        return None;
    }
    let hand_ndc = hand_clip / hand_clip.w;
    let world_clip = world_projection.get_clip_from_view();
    if !world_clip.is_finite() {
        return None;
    }
    let world_view = world_clip.inverse() * hand_ndc;
    if !world_view.is_finite() || world_view.w.abs() < f32::EPSILON {
        return None;
    }
    Some(world_view.truncate() / world_view.w)
}

/// Uses the rendered first-person item rather than the hidden third-person body when the local
/// player casts a fishing line in first person. The item submission is in hand-pass camera space;
/// reproject its sampled rod tip through the world pass before placing the rope.
pub(super) fn first_person_fishing_line_anchor_for(
    source: &HandSource,
    partial_tick: f32,
    camera: &Transform,
    projection: &Projection,
    aspect: f32,
    hand_fov: f32,
) -> Option<FishingLineAnchor> {
    let submissions = source.submissions();
    let submission = submissions.get(1)?.as_ref()?;
    if submission.input.identity.layer != crate::presentation::equipment::LAYER_MAIN_HAND {
        return None;
    }
    let from = fishing_line_anchor_for_submission(submission, partial_tick)?;
    let from = hand_point_to_world_view(from, projection, aspect, hand_fov)?;
    let from = camera.to_matrix().transform_point3(from);
    Some(FishingLineAnchor {
        runtime_id: submission.input.identity.runtime_id,
        from: from.is_finite().then_some(from.to_array())?,
    })
}

/// Checks the actual main-hand item before replacing a fishing line's fallback anchor.
pub(super) fn owner_holds_fishing_rod(
    stream: Option<&chunk_pipeline::WorldStream>,
    runtime_id: u64,
    local_runtime_id: u64,
    local_equipment: &crate::presentation::equipment::ActorEquipmentInput,
) -> bool {
    if runtime_id == local_runtime_id {
        return local_equipment
            .main
            .as_ref()
            .is_some_and(|item| item.identifier.as_ref() == assets::facts::FISHING_ROD_IDENTIFIER);
    }
    stream
        .and_then(|stream| {
            stream
                .authority()
                .actor_equipment_in_hand(runtime_id, protocol::ActorHandedness::Right)
        })
        .is_some_and(|equipment| {
            equipment.item.identifier.as_deref() == Some(assets::facts::FISHING_ROD_IDENTIFIER)
        })
}
