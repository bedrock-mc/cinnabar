//! Seats a riding player for the coming tick.

use sim::{CollisionWorld, MovementInput, PlayerState, Vec3, WorldQueryError};

use crate::movement::ModeIntent;
use crate::movement::vehicle::{VehicleCorrection, VehiclePrediction};

/// Seats the rider on the boat it steers, predicting the boat's tick, or else on the
/// mount's streamed seat. Returns the displacement the seat applied. A steered boat whose
/// terrain is unavailable fails the tick through `tick_error` rather than seating the rider
/// elsewhere.
pub(super) fn seat_rider(
    vehicle: &mut VehiclePrediction,
    state: &mut PlayerState,
    input: &MovementInput,
    intent: &mut ModeIntent,
    world: &impl CollisionWorld,
    tick_error: &mut Option<WorldQueryError>,
) -> Option<[f32; 3]> {
    let [strafe, forward] = input.processed_controls().move_vector;
    let seat = match vehicle.step(intent.vehicle, [strafe as f32, forward as f32], world) {
        // The tick retains the predicted seat, so a player replay seats the rider there.
        Ok(Some(seat)) => Some(*intent.ride_seat.insert(seat)),
        Ok(None) => intent.ride_seat,
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

    /// Restarts the steered boat's prediction from a server correction and returns the
    /// rider's seat feet on it, which the caller snaps the rider to like a teleport.
    pub fn correct_vehicle(&mut self, correction: &VehicleCorrection) -> Option<[f32; 3]> {
        self.vehicle.correct(correction)
    }
}

#[cfg(test)]
mod tests {
    use sim::{Aabb, CollisionQuery};

    use super::*;
    use crate::movement::vehicle::SteeredVehicle;

    /// Solid ground up to y = 0.
    struct Ground;

    impl CollisionWorld for Ground {
        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            let floor = Aabb::new(Vec3::new(-64.0, -4.0, -64.0), Vec3::new(64.0, 0.0, 64.0));
            Ok(CollisionQuery::synthetic(
                floor
                    .intersects(query)
                    .then_some(floor)
                    .into_iter()
                    .collect(),
            ))
        }
    }

    #[test]
    fn a_predicted_seat_is_retained_for_player_replay() {
        let mut intent = ModeIntent {
            vehicle: Some(SteeredVehicle {
                runtime_id: 7,
                unique_id: -7,
                body: sim::BoatBody {
                    width: 1.4,
                    height: 0.455,
                    height_offset: 0.375,
                },
                position: [0.5, 0.375, 0.5],
                yaw: 0.0,
                seat: [0.0, -0.2, 0.0],
                server_moves: 0,
            }),
            // The streamed seat sampled before the tick, which replay must not reuse.
            ride_seat: Some([9.0, 9.0, 9.0]),
            ..ModeIntent::default()
        };
        let mut state = PlayerState::new(Vec3::new(3.0, 0.0, 3.0));
        let mut error = None;
        seat_rider(
            &mut VehiclePrediction::default(),
            &mut state,
            &MovementInput::default(),
            &mut intent,
            &Ground,
            &mut error,
        )
        .unwrap();
        assert_eq!(error, None);
        let seat = intent.ride_seat.unwrap();
        assert_eq!(seat[0], 0.5);
        assert_eq!(state.position.x, f64::from(seat[0]));
        assert_eq!(state.position.y, f64::from(seat[1]));
    }
}
