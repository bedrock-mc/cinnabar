//! Turns the rider's view with its vehicle when its seat locks rotation.

use bevy::prelude::{Res, ResMut};
use client_presentation::{
    actor_publication::ActorFramePartialTick, camera::VehicleLookFollow,
    local_player::LocalViewPose,
};

use super::ClientWorld;

/// Carries the look heading with the vehicle's turn and holds it within the seat's range.
pub(crate) fn follow_vehicle_look(
    client_world: Res<ClientWorld>,
    partial_tick: Res<ActorFramePartialTick>,
    mut follow: ResMut<VehicleLookFollow>,
    mut view: ResMut<LocalViewPose>,
) {
    let lock = client_world
        .stream
        .as_ref()
        .and_then(|stream| stream.authority().local_seat_view_lock(partial_tick.0));
    follow.follow(&mut view, lock);
}
