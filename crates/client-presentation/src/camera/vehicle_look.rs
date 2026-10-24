//! Carries a rider's view with its vehicle's turn and holds it within a locked seat's range.

use bevy::prelude::{EulerRot, Quat, Resource};
use client_world::SeatViewLock;

use crate::local_player::LocalViewPose;
use crate::presentation::actors::wrap_degrees;

/// Follows the vehicle yaw from frame to frame.
#[derive(Debug, Default, Resource)]
pub struct VehicleLookFollow {
    /// The followed vehicle and its yaw last frame.
    last_vehicle: Option<(u64, f32)>,
}

impl VehicleLookFollow {
    /// Degrees to add to the look's Minecraft yaw this frame: the vehicle's turn since the
    /// previous frame, held within the seat's range around its facing. Without a lock the
    /// view is free; the next lock, or a lock on another vehicle, starts without a turn.
    pub fn turn(&mut self, look_yaw: f32, lock: Option<SeatViewLock>) -> f32 {
        let Some(lock) = lock else {
            self.last_vehicle = None;
            return 0.0;
        };
        let vehicle_turn = self
            .last_vehicle
            .filter(|(vehicle, _)| *vehicle == lock.vehicle_runtime_id)
            .map_or(0.0, |(_, last)| wrap_degrees(lock.vehicle_yaw - last));
        self.last_vehicle = Some((lock.vehicle_runtime_id, lock.vehicle_yaw));
        // The look keeps its heading relative to the seat as the vehicle turns, then is held
        // within the seat's range.
        let relative = wrap_degrees(look_yaw - (lock.facing() - vehicle_turn));
        wrap_degrees(lock.facing() + relative.clamp(-lock.limit, lock.limit) - look_yaw)
    }

    /// Turns the view's look heading with the vehicle.
    pub fn follow(&mut self, view: &mut LocalViewPose, lock: Option<SeatViewLock>) {
        let (yaw, pitch, roll) = view.look_rotation().to_euler(EulerRot::YXZ);
        let turn = self.turn(180.0 - yaw.to_degrees(), lock);
        if turn != 0.0 && turn.is_finite() {
            let delta = -turn.to_radians();
            view.turn_look_rotation(
                Quat::from_euler(EulerRot::YXZ, yaw + delta, pitch, roll),
                delta,
            );
        }
    }
}

/// The seat lock turned to the steered boat's predicted heading for this frame, when the
/// prediction is of the lock's vehicle at its current revision; otherwise the lock as the
/// actors report it.
#[must_use]
pub fn predicted_heading(
    mut lock: SeatViewLock,
    predicted: Option<client_world::LocalVehicleFeed>,
    revision: client_world::VehicleRevision,
) -> SeatViewLock {
    if let Some(feed) = predicted
        && feed.runtime_id == lock.vehicle_runtime_id
        && feed.revision == revision
    {
        lock.vehicle_yaw = feed.render_yaw;
    }
    lock
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn boat(vehicle_yaw: f32) -> SeatViewLock {
        SeatViewLock {
            vehicle_runtime_id: 7,
            vehicle_yaw,
            offset: -90.0,
            limit: 90.0,
        }
    }

    #[test]
    fn the_view_turns_with_the_vehicle() {
        let mut follow = VehicleLookFollow::default();
        assert_eq!(follow.turn(-90.0, Some(boat(0.0))), 0.0);
        assert_eq!(follow.turn(-90.0, Some(boat(15.0))), 15.0);
        // Turning across the wrap point is still a small turn.
        let mut follow = VehicleLookFollow::default();
        follow.turn(85.0, Some(boat(175.0)));
        assert!((follow.turn(85.0, Some(boat(-175.0))) - 10.0).abs() < 1.0e-4);
    }

    /// A large vehicle turn carries the view the same way, not to the far edge of the seat.
    #[test]
    fn a_large_vehicle_turn_carries_the_view_with_it() {
        let mut follow = VehicleLookFollow::default();
        let seat = |vehicle_yaw| SeatViewLock {
            offset: 0.0,
            ..boat(vehicle_yaw)
        };
        assert_eq!(follow.turn(-90.0, Some(seat(0.0))), 0.0);
        assert!((follow.turn(-90.0, Some(seat(100.0))) - 100.0).abs() < 1.0e-4);
    }

    /// Moving straight to another locked vehicle does not turn the view by the difference
    /// between the two vehicles' headings.
    #[test]
    fn a_new_vehicle_starts_without_a_turn() {
        let mut follow = VehicleLookFollow::default();
        follow.turn(-90.0, Some(boat(0.0)));
        let other = SeatViewLock {
            vehicle_runtime_id: 8,
            ..boat(90.0)
        };
        assert_eq!(follow.turn(0.0, Some(other)), 0.0);
    }

    /// Only a prediction of the locked vehicle at its current revision sets its heading.
    #[test]
    fn a_prediction_of_another_vehicle_leaves_the_lock_alone() {
        let feed = client_world::LocalVehicleFeed {
            runtime_id: 7,
            revision: client_world::VehicleRevision { pose: 3, motion: 0 },
            position: [0.0; 3],
            yaw: 40.0,
            velocity: [0.0; 3],
            render_position: [0.0; 3],
            render_yaw: 35.0,
        };
        let lock = boat(10.0);
        let current = client_world::VehicleRevision { pose: 3, motion: 0 };
        assert_eq!(
            predicted_heading(lock, Some(feed), current).vehicle_yaw,
            35.0
        );
        let other = client_world::LocalVehicleFeed {
            runtime_id: 8,
            ..feed
        };
        assert_eq!(predicted_heading(lock, Some(other), current), lock);
        let newer = client_world::VehicleRevision { pose: 4, motion: 0 };
        assert_eq!(predicted_heading(lock, Some(feed), newer), lock);
        assert_eq!(predicted_heading(lock, None, current), lock);
    }

    #[test]
    fn the_view_stays_within_the_seat_range() {
        let mut follow = VehicleLookFollow::default();
        // Looking 120 degrees off the seat's facing is pulled back to the 90 degree edge.
        assert_eq!(follow.turn(30.0, Some(boat(0.0))), -30.0);
        assert_eq!(follow.turn(-190.0, Some(boat(0.0))), 10.0);
    }

    #[test]
    fn an_unlocked_seat_leaves_the_view_free() {
        let mut follow = VehicleLookFollow::default();
        follow.turn(0.0, Some(boat(0.0)));
        assert_eq!(follow.turn(0.0, None), 0.0);
        // A new lock starts without carrying the old vehicle yaw.
        assert_eq!(follow.turn(-90.0, Some(boat(40.0))), 0.0);
    }
}
