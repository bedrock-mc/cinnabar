//! Carries a rider's view with its vehicle's turn and holds it within a locked seat's range.

use bevy::prelude::{EulerRot, Quat, Resource};
use client_world::SeatViewLock;

use crate::local_player::LocalViewPose;
use crate::presentation::actors::wrap_degrees;

/// Follows the vehicle yaw from frame to frame.
#[derive(Debug, Default, Resource)]
pub struct VehicleLookFollow {
    last_vehicle_yaw: Option<f32>,
}

impl VehicleLookFollow {
    /// Degrees to add to the look's Minecraft yaw this frame: the vehicle's turn since the
    /// previous frame, held within the seat's range around its facing. Without a lock the
    /// view is free and the next lock starts without a turn.
    pub fn turn(&mut self, look_yaw: f32, lock: Option<SeatViewLock>) -> f32 {
        let Some(lock) = lock else {
            self.last_vehicle_yaw = None;
            return 0.0;
        };
        let vehicle_turn = self
            .last_vehicle_yaw
            .map_or(0.0, |last| wrap_degrees(lock.vehicle_yaw - last));
        self.last_vehicle_yaw = Some(lock.vehicle_yaw);
        let relative = wrap_degrees(look_yaw - lock.facing);
        (relative + vehicle_turn).clamp(-lock.limit, lock.limit) - relative
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

#[cfg(test)]
mod tests {
    use super::*;

    const fn boat(vehicle_yaw: f32) -> SeatViewLock {
        SeatViewLock {
            vehicle_yaw,
            facing: vehicle_yaw - 90.0,
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
