//! Shared submission eligibility for draw construction and early hand readiness.

use super::*;
use crate::{ActorRigRejects, ActorRigSubmission};

pub(super) enum Rejection {
    NoDraw,
    Identity,
    Transform,
    Length,
    Capacity,
    Pose,
    MissingGeometry,
    Geometry,
}

impl Rejection {
    /// Counts the same rejection for normal frame construction.
    pub(super) fn count(self, rejects: &mut ActorRigRejects) {
        let count = match self {
            Self::NoDraw => &mut rejects.no_draw,
            Self::Identity => &mut rejects.invalid_identity,
            Self::Transform => &mut rejects.invalid_world_transform,
            Self::Length => &mut rejects.pose_length_mismatch,
            Self::Capacity => &mut rejects.bone_capacity,
            Self::Pose => &mut rejects.non_finite_pose,
            Self::MissingGeometry => &mut rejects.missing_geometry,
            Self::Geometry => &mut rejects.invalid_geometry,
        };
        *count = count.saturating_add(1);
    }
}

/// Rejects missing identity and placement before culling or pose work.
pub(super) fn validate_input(submission: &ActorRigSubmission) -> Result<(), Rejection> {
    if submission.route == ActorRigRoute::NoDraw {
        return Err(Rejection::NoDraw);
    }
    let diagnostic = submission.route == ActorRigRoute::Diagnostic;
    if (!submission.input.identity.is_exact() && !diagnostic)
        || submission.input.reset_generation == 0
    {
        return Err(Rejection::Identity);
    }
    if submission
        .world_from_actor
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(Rejection::Transform);
    }
    Ok(())
}

/// Resolves exactly the geometry and finite pose admitted by the normal rig builder. A pose
/// whose matrices `matrices` already holds for the resolved geometry is known finite and is not
/// rescanned; rejections keep their order either way.
pub(super) fn geometry<'a>(
    geometries: &'a BTreeMap<EntityRigId, ActorRigGeometry>,
    matrices: &PoseMatrixCache,
    submission: &ActorRigSubmission,
) -> Result<(EntityRigId, &'a ActorRigGeometry), Rejection> {
    let previous = &submission.input.previous_bones;
    let current = &submission.input.current_bones;
    if previous.len() != current.len() {
        return Err(Rejection::Length);
    }
    if previous.is_empty() || previous.len() > MAX_RENDER_BONES_PER_ACTOR {
        return Err(Rejection::Capacity);
    }
    let id = match submission.route {
        ActorRigRoute::Compiled | ActorRigRoute::StaticFallback => Some(submission.input.rig),
        ActorRigRoute::Diagnostic => Some(DIAGNOSTIC_RIG_ID),
        ActorRigRoute::NoDraw => None,
    };
    let finite = |pose: &Arc<[RenderBoneTransform]>| {
        id.is_some_and(|id| matrices.holds_finite(pose, id))
            || pose.iter().all(|bone| bone.is_finite())
    };
    if !finite(previous) || (!Arc::ptr_eq(previous, current) && !finite(current)) {
        return Err(Rejection::Pose);
    }
    let Some(id) = id else {
        return Err(Rejection::NoDraw);
    };
    let geometry = geometries.get(&id).ok_or(Rejection::MissingGeometry)?;
    if geometry.bones_used() > previous.len() {
        return Err(Rejection::Geometry);
    }
    Ok((id, geometry))
}
