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

/// Where the rider renders from this tick. On a boat the client predicts, that is where it
/// was before this tick seated it, so it eases along with the boat and a failed tick returns
/// it there. On a streamed mount it is the seat itself, which the mount's own rendering
/// already reached.
pub(super) fn before_seat(position: Vec3, ride_delta: Option<[f32; 3]>, predicted: bool) -> Vec3 {
    ride_delta
        .filter(|_| predicted)
        .map_or(position, |[x, y, z]| {
            position - Vec3::new(f64::from(x), f64::from(y), f64::from(z))
        })
}

impl super::LocalPhysicsController {
    /// The predicted pose of the boat the player steers, for the actor store.
    #[must_use]
    pub fn predicted_vehicle_feed(&self) -> Option<client_world::LocalVehicleFeed> {
        self.vehicle.feed(self.tick_alpha())
    }

    /// Ends render easing at the committed state, for the eye, visual corrections and the
    /// steered boat alike, so a blocked frame shows the rider on its boat.
    pub(super) fn finish_render_interpolation(&mut self) {
        self.eye_offset.finish_interpolation();
        self.visual_correction.finish_interpolation();
        self.vehicle.finish_interpolation();
    }

    /// Forgets the steered boat's prediction; the next steered tick starts a new one. A new
    /// session's boat, even one with the old ids, starts its own prediction.
    pub fn end_vehicle_prediction(&mut self) {
        self.vehicle.reset();
    }

    /// Restarts the steered boat's prediction from a server correction and returns the
    /// rider's seat feet on it, which the caller snaps the rider to like a teleport.
    pub fn correct_vehicle(&mut self, correction: &VehicleCorrection) -> Option<[f32; 3]> {
        self.vehicle.correct(correction)
    }
}

#[cfg(test)]
impl super::LocalPhysicsController {
    /// Predicts `vehicle` as if its ticks through `completed` already ran.
    pub(crate) fn steer_completed(
        &mut self,
        vehicle: crate::movement::vehicle::SteeredVehicle,
        completed: u64,
    ) {
        self.vehicle = VehiclePrediction::completed_session(vehicle, completed);
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

    /// [`Ground`] whose terrain beyond x = 100 is unloaded.
    struct EdgeGround;

    impl CollisionWorld for EdgeGround {
        fn ensure_movement_origin(&self, feet: Vec3) -> Result<(), WorldQueryError> {
            if feet.x >= 100.0 {
                return Err(WorldQueryError::UnloadedChunk(world::ChunkKey {
                    dimension: 0,
                    x: 6,
                    z: 0,
                }));
            }
            Ok(())
        }

        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Ground.collision_boxes(query)
        }
    }

    /// When the boat's step succeeds but the rider's tick fails, the rider stays where the
    /// committed state left it rather than on the discarded seat.
    /// A replacement StartGame ends the previous session's boat prediction.
    #[test]
    fn start_game_ends_the_previous_session_s_boat_prediction() {
        let mut physics = super::super::LocalPhysicsController::default();
        physics.steer_completed(
            SteeredVehicle {
                runtime_id: 7,
                unique_id: -7,
                body: sim::BoatBody {
                    width: 1.4,
                    height: 0.455,
                    height_offset: 0.375,
                },
                position: [0.5, 0.375, 0.5],
                yaw: 0.0,
                velocity: [0.0; 3],
                seat: [0.0; 3],
                revision: client_world::VehicleRevision { pose: 0, motion: 0 },
            },
            100,
        );
        assert!(physics.predicted_vehicle_feed().is_some());
        crate::movement::bootstrap::reset_start_game_prediction(
            &mut crate::movement::MovementTicker::default(),
            &mut physics,
            8,
            [0.5, 2.0, 0.5],
        );
        assert_eq!(physics.predicted_vehicle_feed(), None);
    }

    #[test]
    fn a_failed_tick_leaves_the_rider_off_the_uncommitted_seat() {
        let mut physics = super::super::LocalPhysicsController::default();
        physics.reanchor_network_position([99.95, protocol::PLAYER_NETWORK_OFFSET, 0.5], 100, true);
        let ride = crate::movement::PhysicsSampleContext {
            mode_intent: ModeIntent {
                ride: Some(crate::movement::RideKind::Boat),
                vehicle: Some(SteeredVehicle {
                    runtime_id: 7,
                    unique_id: -7,
                    body: sim::BoatBody {
                        width: 1.4,
                        height: 0.455,
                        height_offset: 0.375,
                    },
                    position: [99.95, 0.375, 0.5],
                    yaw: 0.0,
                    velocity: [0.6, 0.0, 0.0],
                    seat: [0.0, 0.0, 0.0],
                    revision: client_world::VehicleRevision { pose: 0, motion: 0 },
                }),
                ..ModeIntent::default()
            },
            ..Default::default()
        };
        let frame = physics.advance_with_context(
            std::time::Duration::from_millis(50),
            MovementInput::default(),
            ride,
            &EdgeGround,
        );
        assert!(frame.blocked.is_some());
        assert_eq!(physics.network_position().unwrap()[0], 99.95);
    }

    /// A frame blocked on unloaded terrain shows the boat at its committed state, where the
    /// rider is held, not one tick behind it.
    #[test]
    fn a_blocked_frame_shows_the_boat_under_its_held_rider() {
        let mut physics = super::super::LocalPhysicsController::default();
        physics.reanchor_network_position([99.0, protocol::PLAYER_NETWORK_OFFSET, 0.5], 100, true);
        let mut vehicle = SteeredVehicle {
            runtime_id: 7,
            unique_id: -7,
            body: sim::BoatBody {
                width: 1.4,
                height: 0.455,
                height_offset: 0.375,
            },
            position: [99.0, 0.375, 0.5],
            yaw: 0.0,
            velocity: [0.3, 0.0, 0.0],
            seat: [0.0, 0.0, 0.0],
            revision: Default::default(),
        };
        let mut committed = false;
        for _ in 0..20 {
            let ride = crate::movement::PhysicsSampleContext {
                mode_intent: ModeIntent {
                    ride: Some(crate::movement::RideKind::Boat),
                    vehicle: Some(vehicle),
                    ..ModeIntent::default()
                },
                ..Default::default()
            };
            let frame = physics.advance_with_context(
                std::time::Duration::from_millis(50),
                MovementInput::default(),
                ride,
                &EdgeGround,
            );
            if frame.blocked.is_some() {
                let feed = physics.predicted_vehicle_feed().unwrap();
                assert!(committed);
                assert_eq!(feed.render_position, feed.position);
                return;
            }
            committed = true;
            let feed = physics.predicted_vehicle_feed().unwrap();
            vehicle.position = [feed.position[0], feed.position[1] + 0.375, feed.position[2]];
        }
        panic!("the boat never reached the unloaded terrain");
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
                velocity: [0.0; 3],
                seat: [0.0, -0.2, 0.0],
                revision: client_world::VehicleRevision { pose: 0, motion: 0 },
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
