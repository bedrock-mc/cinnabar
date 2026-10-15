use super::{PhysicsAuthorityFault, physics::is_retryable_collision_block};

/// Classifies a failed frame while retaining authority for terrain waits and query limits.
pub fn physics_authority_fault_for_frame(
    frame: &super::LocalPhysicsFrame,
) -> Option<PhysicsAuthorityFault> {
    if frame
        .blocked
        .as_ref()
        .is_some_and(is_retryable_collision_block)
    {
        return None;
    }

    let error = frame.blocked.as_ref()?;
    Some(PhysicsAuthorityFault::PhysicsSimulationError {
        due: frame.due_ticks,
        tick_index: frame.blocked_tick_index.unwrap_or(frame.completed_ticks),
        error: error.clone(),
    })
}
