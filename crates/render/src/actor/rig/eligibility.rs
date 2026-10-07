//! Shared submission eligibility for draw construction and early hand readiness.
use super::*;

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
    let diagnostic = submission.route == ActorRigRoute::Diagnostic
        || (submission.route == ActorRigRoute::ShadowOnly
            && submission.input.rig == DIAGNOSTIC_RIG_ID);
    if ((!submission.input.identity.is_exact() || submission.input.completed_tick == 0)
        && !diagnostic)
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

/// Resolves exactly the geometry and finite pose admitted by the normal rig builder.
pub(super) fn geometry<'a>(
    catalog: &'a GeometryCatalog,
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
    if previous
        .iter()
        .chain(current.iter())
        .any(|bone| !bone.is_finite())
    {
        return Err(Rejection::Pose);
    }
    let id = match submission.route {
        ActorRigRoute::Compiled | ActorRigRoute::StaticFallback | ActorRigRoute::ShadowOnly => {
            submission.input.rig
        }
        ActorRigRoute::Diagnostic => DIAGNOSTIC_RIG_ID,
        ActorRigRoute::NoDraw => return Err(Rejection::NoDraw),
    };
    let geometry = catalog
        .geometries
        .get(&id)
        .ok_or(Rejection::MissingGeometry)?;
    if geometry.bones_used() > previous.len() {
        return Err(Rejection::Geometry);
    }
    Ok((id, geometry))
}
