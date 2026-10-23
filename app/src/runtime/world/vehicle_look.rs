//! Turns the rider's view with its vehicle when its seat locks rotation.

use bevy::prelude::{Res, ResMut};
use client_interaction::movement::LocalPhysicsController;
use client_presentation::{
    actor_publication::{ActorFramePartialTick, ActorFrameState},
    camera::VehicleLookFollow,
    local_player::LocalViewPose,
};

use super::ClientWorld;

/// Carries the look heading with the vehicle's turn this frame and holds it within the
/// seat's range. A boat the player steers turns at its predicted heading for this frame;
/// any other vehicle at the streamed heading this frame's actor step reaches.
pub(crate) fn follow_vehicle_look(
    client_world: Res<ClientWorld>,
    physics: Res<LocalPhysicsController>,
    actor_frame: Res<ActorFrameState>,
    partial_tick: Res<ActorFramePartialTick>,
    mut follow: ResMut<VehicleLookFollow>,
    mut view: ResMut<LocalViewPose>,
) {
    let alpha = actor_frame
        .motion_step()
        .map_or(partial_tick.0, |step| step.partial_tick);
    let lock = client_world
        .stream
        .as_ref()
        .and_then(|stream| stream.authority().local_seat_view_lock(alpha))
        .map(|mut lock| {
            if let Some(yaw) = physics.predicted_vehicle_yaw() {
                lock.vehicle_yaw = yaw;
            }
            lock
        });
    follow.follow(&mut view, lock);
}
