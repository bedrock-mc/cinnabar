use super::{PhysicsAuthorityFault, physics::is_transient_collision_unavailability};

/// Classifies a failed simulation frame without treating pending world data as a fault.
pub fn physics_authority_fault_for_frame(
    frame: &super::LocalPhysicsFrame,
) -> Option<PhysicsAuthorityFault> {
    if frame
        .blocked
        .as_ref()
        .is_some_and(is_transient_collision_unavailability)
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
