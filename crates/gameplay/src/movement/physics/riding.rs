//! Seats a riding player for the coming tick.

use sim::{CollisionWorld, MovementInput, PlayerState, Vec3, WorldQueryError};

use crate::movement::ModeIntent;
use crate::movement::vehicle::{VehicleCorrection, VehiclePrediction, VehicleReplay};

/// Seats the rider on the boat it steers, predicting the boat's tick, or else on the
/// mount's streamed seat. Returns the displacement the seat applied. A steered boat whose
/// terrain is unavailable fails the tick through `tick_error` rather than seating the rider
/// elsewhere.
pub(super) fn seat_rider(
    vehicle: &mut VehiclePrediction,
    state: &mut PlayerState,
    input: &MovementInput,
    intent: &ModeIntent,
    world: &impl CollisionWorld,
    tick_error: &mut Option<WorldQueryError>,
) -> Option<[f32; 3]> {
    let [strafe, forward] = input.processed_controls().move_vector;
    let seat = match vehicle.step(intent.vehicle, [strafe as f32, forward as f32], world) {
        Ok(seat) => seat.or(intent.ride_seat),
        Err(error) => {
            tick_error.get_or_insert(error);
            return None;
        }
    }
    .filter(|seat| seat.iter().all(|axis| axis.is_finite()));
    seat.map(|seat| {
        let seat = Vec3::new(f64::from(seat[0]), f64::from(seat[1]), f64::from(seat[2]));
        let delta = [
            (seat.x - state.position.x) as f32,
            (seat.y - state.position.y) as f32,
            (seat.z - state.position.z) as f32,
        ];
        state.position = seat;
        delta
    })
}

impl super::LocalPhysicsController {
    /// The predicted pose of the boat the player steers, for the actor store.
    #[must_use]
    pub fn predicted_vehicle_feed(&self) -> Option<client_world::LocalVehicleFeed> {
        self.vehicle.feed()
    }

    /// Applies a server correction of the steered boat, reseats the rider on it and updates
    /// the retained samples of the replayed ticks. Returns the replay so unsent inputs can be
    /// reconciled, or `None` when nothing changed.
    pub fn correct_vehicle(
        &mut self,
        correction: &VehicleCorrection,
        world: &impl CollisionWorld,
    ) -> Option<VehicleReplay> {
        let replay = self.vehicle.correct(correction, world)?;
        for sample in &mut self.sample_history {
            if let Some((_, corrected)) =
                replay.samples.iter().find(|(tick, _)| *tick == sample.tick)
                && sample.processed.vehicle.is_some()
            {
                sample.processed.vehicle = Some(*corrected);
            }
        }
        if let Some(state) = self.state.as_mut()
            && replay.seat.iter().all(|axis| axis.is_finite())
        {
            let [x, y, z] = replay.seat;
            state.position = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
        }
        Some(replay)
    }
}
