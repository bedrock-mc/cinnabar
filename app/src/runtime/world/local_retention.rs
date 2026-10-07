use chunk_pipeline::WorldStream;
use client_presentation::local_player_camera_receipt::{CameraOwner, CameraPublicationAttempt};

use crate::movement::LocalPhysicsController;

/// Uses completed physics only after this frame proves its current session and control ownership.
/// Without local physics (free camera or a faulted authority) the server position owns the grid.
pub(super) fn retain_completed_player_terrain(
    stream: &mut WorldStream,
    physics: &LocalPhysicsController,
    publication: Option<&CameraPublicationAttempt>,
    session_generation: u64,
) -> bool {
    if !physics.is_active() {
        return stream.retain_for_server_position();
    }
    let Some(published) = publication.and_then(CameraPublicationAttempt::published) else {
        return false;
    };
    let owner = CameraOwner::current(stream, session_generation);
    if published.owner != owner
        || physics
            .state()
            .is_none_or(|state| state.tick != published.tick)
    {
        return false;
    }
    let Some(position) = physics.network_position() else {
        return false;
    };
    stream.retain_for_local_player(owner.stream, owner.dimension, owner.epoch, position)
}

#[cfg(test)]
mod tests;
