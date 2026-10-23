//! Seats a riding player for the coming tick.

use sim::{CollisionWorld, MovementInput, PlayerState, Vec3};

use crate::movement::ModeIntent;
use crate::movement::vehicle::VehiclePrediction;

/// Seats the rider on the boat it steers, predicting the boat's tick, or else on the
/// mount's streamed seat. Returns the displacement the seat applied.
pub(super) fn seat_rider(
    vehicle: &mut VehiclePrediction,
    state: &mut PlayerState,
    input: &MovementInput,
    intent: &ModeIntent,
    world: &impl CollisionWorld,
) -> Option<[f32; 3]> {
    let [strafe, forward] = input.processed_controls().move_vector;
    let seat = vehicle
        .step(intent.vehicle, [strafe as f32, forward as f32], world)
        .or(intent.ride_seat)
        .filter(|seat| seat.iter().all(|axis| axis.is_finite()))?;
    let seat = Vec3::new(f64::from(seat[0]), f64::from(seat[1]), f64::from(seat[2]));
    let delta = [
        (seat.x - state.position.x) as f32,
        (seat.y - state.position.y) as f32,
        (seat.z - state.position.z) as f32,
    ];
    state.position = seat;
    Some(delta)
}

impl super::LocalPhysicsController {
    /// The predicted pose of the boat the player steers, for the actor store.
    #[must_use]
    pub fn predicted_vehicle_feed(&self) -> Option<client_world::LocalVehicleFeed> {
        self.vehicle.feed()
    }

    /// Applies a server correction of the steered boat and reseats the rider on it.
    pub fn correct_vehicle(
        &mut self,
        correction: &crate::movement::vehicle::VehicleCorrection,
        world: &impl CollisionWorld,
    ) {
        let Some(seat) = self.vehicle.correct(correction, world) else {
            return;
        };
        if let Some(state) = self.state.as_mut()
            && seat.iter().all(|axis| axis.is_finite())
        {
            state.position = Vec3::new(f64::from(seat[0]), f64::from(seat[1]), f64::from(seat[2]));
        }
    }
}
